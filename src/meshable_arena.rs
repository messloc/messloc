use crate::arena_fs::open_shm_span_file;
use crate::bitmap::{Bitmap, BitmapBase, RelaxedBitmapBase};
use crate::runtime::Runtime;
use crate::comparatomic::Comparatomic;
use crate::span::{Length, Offset, Span, SpanList};
use crate::{
    cheap_heap::CheapHeap,
    mini_heap::{AtomicMiniHeapId, MiniHeap},
    one_way_mmap_heap::Heap,
    ARENA_SIZE, DEFAULT_MAX_MESH_COUNT, DIRTY_PAGE_THRESHOLD, MIN_ARENA_EXPANSION, PAGE_SIZE,
    SPAN_CLASS_COUNT,
};
use std::mem::size_of;
use std::{
    path::PathBuf,
    ptr::null_mut,
    sync::atomic::{AtomicPtr, Ordering},
    sync::{Arc, Mutex},
};

use crate::{utils::*, MAP_SHARED};
use libc::c_void;
pub type Page = [u8; PAGE_SIZE];

pub struct MeshableArena<'a> {
    pub runtime: Runtime<'a>,
    pub(crate) arena_begin: *mut Page,
    fd: i32,
    /// offset in pages
    end: Offset,
    dirty: SpanList,
    clean: SpanList,
    freed_spans: SpanList,
    mh_index: [AtomicMiniHeapId<CheapHeap<64, { ARENA_SIZE / PAGE_SIZE }>>; ARENA_SIZE / PAGE_SIZE],
    pub(crate) mh_allocator: CheapHeap<64, { ARENA_SIZE / PAGE_SIZE }>,
    meshed_bitmap: Bitmap<RelaxedBitmapBase<{ ARENA_SIZE / PAGE_SIZE }>>,
    fork_pipe: [i32; 2],
    span_dir: PathBuf,
    meshed_page_count_hwm: u64,
    max_mesh_count: usize,
}

unsafe impl Sync for MeshableArena<'_> {}
unsafe impl Send for MeshableArena<'_> {}



impl<'a> MeshableArena<'a> {
    pub fn init() -> MeshableArena<'a> {
        // TODO: check if meshing enabled
        //TODO: initialise stuff from the constructor

        let fd = open_shm_span_file(ARENA_SIZE);

        let mut mh_allocator = CheapHeap::new();
        let arena_begin =
            unsafe { mh_allocator.map(ARENA_SIZE, MAP_SHARED, fd).inner() as *mut Page };

        let mh_index = unsafe { mh_allocator.malloc(index_size()) };

        MeshableArena {
            runtime: Runtime::init(),
            //TODO:: find initial end value
            arena_begin,
            fd,
            end: Offset::default(),
            dirty: SpanList::default(),
            clean: SpanList::default(),
            mh_allocator,
            mh_index,
            meshed_bitmap: Bitmap::default(),
            freed_spans: SpanList::default(),
            fork_pipe: [-1, -1],
            span_dir: PathBuf::default(),
            meshed_page_count_hwm: 0,
            max_mesh_count: DEFAULT_MAX_MESH_COUNT,
        }
    }

    pub fn page_alloc(&mut self, page_count: usize, page_align: usize) -> (Span, *mut Page) {
        if page_count == 0 {
            return (Span::default(), null_mut());
        }

        debug_assert_ne!(
            self.arena_begin,
            null_mut(),
            "meshable arena must be initialised"
        );

        debug_assert!(page_count > 0);
        debug_assert!(page_count < Length::MAX as usize);

        let span = self.reserve_pages(page_count, page_align);

        //     d_assert(isAligned(span, pageAlignment));
        //     d_assert(contains(ptrFromOffset(span.offset)));
        //   #ifndef NDEBUG
        //     if (_mhIndex[span.offset].load().hasValue()) {
        //       mesh::debug("----\n");
        //       auto mh = reinterpret_cast<MiniHeap *>(miniheapForArenaOffset(span.offset));
        //       mh->dumpDebug();
        //     }
        //   #endif

        // Safety: the span offset resides within our total arena size
        let ptr = unsafe { self.arena_begin.add(span.offset as usize) };

        //     if (kAdviseDump) {
        //       madvise(ptr, pageCount * kPageSize, MADV_DODUMP);
        //     }

        (span, ptr)
    }

    fn reserve_pages(&mut self, page_count: usize, page_align: usize) -> Span {
        debug_assert!(page_count > 0);

        let (result, flags) = match self.find_pages(page_count as u32) {
            Some((span, flags)) => (span, flags),
            None => {
                self.expand_arena(page_count);
                self.find_pages(page_count as u32).unwrap() // unchecked?
            }
        };

        debug_assert!(!result.is_empty());
        debug_assert_ne!(flags, PageType::Unknown);

        let ptr = unsafe { self.arena_begin.add(result.offset as usize) };
        if page_align > 1 && ptr.align_offset(page_align) != 0 {
            todo!("page wasn't aligned")

            // freeSpan(result, flags);
            // // recurse once, asking for enough extra space that we are sure to
            // // be able to find an aligned offset of pageCount pages within.
            // result = reservePages(pageCount + 2 * pageAlignment, 1);

            // const size_t alignment = pageAlignment * kPageSize;
            // const uintptr_t alignedPtr = (ptrvalFromOffset(result.offset) + alignment - 1) & ~(alignment - 1);
            // const auto alignedOff = offsetFor(reinterpret_cast<void *>(alignedPtr));
            // d_assert(alignedOff >= result.offset);
            // d_assert(alignedOff < result.offset + result.length);
            // const auto unwantedPageCount = alignedOff - result.offset;
            // auto alignedResult = result.splitAfter(unwantedPageCount);
            // d_assert(alignedResult.offset == alignedOff);
            // freeSpan(result, flags);
            // const auto excess = alignedResult.splitAfter(pageCount);
            // freeSpan(excess, flags);
            // result = alignedResult;
        }

        result
    }

    pub fn expand_arena(&mut self, min_pages_added: usize) {
        let page_count = usize::max(min_pages_added, MIN_ARENA_EXPANSION);

        let expansion = Span {
            offset: self.end,
            length: page_count as u32,
        };
        self.end += page_count as u32;

        //   if (unlikely(_end >= kArenaSize / kPageSize)) {
        //     debug("Mesh: arena exhausted: current arena size is %.1f GB; recompile with larger arena size.",
        //           kArenaSize / 1024.0 / 1024.0 / 1024.0);
        //     abort();
        //   }

        self.clean.inner_mut()[expansion.class() as usize].push(expansion)

        //   _clean[expansion.spanClass()].push_back(expansion);
    }

    fn find_pages(&mut self, page_count: u32) -> Option<(Span, PageType)> {
        // Search through all dirty spans first.  We don't worry about
        // fragmenting dirty pages, as being able to reuse dirty pages means
        // we don't increase RSS.
        let span = Span {
            offset: 0,
            length: page_count,
        };

        for span_class in span.class()..SPAN_CLASS_COUNT {
            if let Some(span) = Self::find_pages_inner(&mut self.dirty, span_class, page_count) {
                return Some((span, PageType::Dirty));
            }
        }

        // if no dirty pages are available, search clean pages.  An allocated
        // clean page (once it is written to) means an increased RSS.
        for span_class in span.class()..SPAN_CLASS_COUNT {
            if let Some(span) = Self::find_pages_inner(&mut self.clean, span_class, page_count) {
                return Some((span, PageType::Clean));
            }
        }

        None
    }

    fn find_pages_inner(
        free_spans: &mut SpanList,
        span_class: u32,
        page_count: u32,
    ) -> Option<Span> {
        let spans = free_spans.get_mut(span_class as usize).unwrap();
        if spans.is_empty() {
            return None;
        }

        // let old_len = spans.len();
        let end = spans.len() - 1;

        if span_class == SPAN_CLASS_COUNT - 1 && spans[end].length < page_count {
            // the final span class contains (and is the only class to
            // contain) variable-size spans, so we need to make sure we
            // search through all candidates in this case.

            let mut iter = spans
                .iter_mut()
                .enumerate()
                .skip_while(|(_, span)| span.length < page_count);

            if let Some((j, _)) = iter.next() {
                spans.swap(j, end);
            } else {
                return None;
            };
        }

        let mut span = spans.pop().unwrap();

        // #ifndef NDEBUG
        // d_assert_msg(oldLen == spanList.size() + 1, "pageCount:%zu,%zu -- %zu/%zu", pageCount, i, oldLen, spanList.size());
        // for (size_t j = 0; j < spanList.size(); j++) {
        // d_assert(spanList[j] != span);
        // }
        // #endif

        // invariant should be maintained
        debug_assert!(span.length > span_class);
        debug_assert!(span.length >= page_count);

        // put the part we don't need back in the reuse pile
        let rest = span.split_after(page_count);
        if !rest.is_empty() {
            free_spans
                .get_mut(rest.class() as usize)
                .unwrap()
                .push(rest);
        }
        debug_assert_eq!(span.length, page_count);

        Some(span)
    }

    pub unsafe fn track_miniheap(
        &mut self,
        span: Span,
        id: *mut CheapHeap<64, { ARENA_SIZE / PAGE_SIZE }>,
    ) {
        for i in 0..span.length {
            self.set_index((span.offset + i) as usize, id)
        }
    }

    pub unsafe fn set_index(
        &mut self,
        offset: usize,
        id: *mut CheapHeap<64, { ARENA_SIZE / PAGE_SIZE }>,
    ) {
        self.mh_index
            .get(offset)
            .unwrap()
            .store(id, Ordering::Release);
    }

    pub fn set_max_mesh_count(&mut self, max_mesh_count: usize) {
        self.max_mesh_count = max_mesh_count;
    }

    pub fn scavenge(&mut self, force: bool) {
        if force && self.dirty.len() < DIRTY_PAGE_THRESHOLD {
            let mut bitmap: Bitmap<RelaxedBitmapBase<{ ARENA_SIZE / PAGE_SIZE }>> =
                Bitmap::default();

            bitmap.invert();

            self.freed_spans.inner().iter().for_each(|span_list| {
                span_list.iter().enumerate().for_each(|(key, span)| {
                    self.meshed_bitmap.unset(span.offset as usize + key);
                    (0..=span.length).for_each(|k| {
                        bitmap.try_to_set(usize::try_from(span.offset + k).unwrap());
                    });
                    let ptr =
                        unsafe { self.arena_begin.add(usize::try_from(span.offset).unwrap()) };
                    reset_span_mapping(ptr as *mut c_void, self.fd, span);
                })
            });

            self.freed_spans.clear();

            let page_count = self.meshed_bitmap.in_use_count();
            if page_count > self.meshed_page_count_hwm {
                self.meshed_page_count_hwm = page_count;
            }

            self.dirty.for_each_free(|span| {
                let size = span.byte_length();
                free_physical(self.fd, span.offset as usize, size);
                (0..=span.length).for_each(|k| {
                    bitmap.try_to_set(usize::try_from(span.offset + k).unwrap());
                })
            });

            self.dirty.clear();
            self.clean.clear();

            self.coalesce(bitmap);
        }
    }

    fn coalesce<const N: usize>(&mut self, bitmap: Bitmap<RelaxedBitmapBase<N>>) {
        let mut current = Span::default();
        for i in bitmap.inner().bits().iter() {
            if *i == (current.offset + current.length) as u64 {
                current.length += 1;
                continue;
            }

            if !current.is_empty() {
                self.clean
                    .inner_mut()
                    .get_mut(current.class() as usize)
                    .unwrap()
                    .push(current);
            }

            current = Span::new(u32::try_from(*i).unwrap(), 1);
        }
    }

    fn partial_scavenge(&mut self) {
        self.dirty.for_each_free(|span| {
            let ptr = unsafe { self.arena_begin.add(span.offset as usize) };
            let size = span.byte_length();
            unsafe { madvise(ptr as *mut libc::c_void, size) }.unwrap();
            free_physical(self.fd, span.offset as usize, size);
            self.clean
                .get_mut(span.class() as usize)
                .unwrap()
                .push(span.clone());
        });

        self.dirty.clear();
    }

    pub fn begin_mesh(&self, remove: *mut c_void, size: usize) {
        unsafe { mprotect_read(remove, size).unwrap() };
    }

    fn finalise_mesh(&mut self, keep: *mut (), remove: *mut (), size: usize) {
        let keep_offset = unsafe { keep.offset_from(self.arena_begin as *mut ()) };
        let remove_offset = unsafe { remove.offset_from(self.arena_begin as *mut ()) };
        let page_count = size / PAGE_SIZE;
        self.store_indices(usize::try_from(keep_offset).unwrap(), page_count);
        let removed_span = Span::new(
            u32::try_from(remove_offset).unwrap(),
            u32::try_from(page_count).unwrap(),
        );
        self.meshed_bitmap.track_meshed(removed_span);
        let _ = unsafe {
            mmap(
                remove as *mut c_void,
                self.fd,
                size,
                usize::try_from(keep_offset).unwrap() * PAGE_SIZE,
            )
        };
    }
    fn store_indices(&mut self, keep_offset: usize, page_count: usize) {
        let keep_id = unsafe { self.mh_index.get_mut(keep_offset).unwrap().get(keep_offset) };
        (0..page_count).for_each(|index| {
            self.mh_index
                .get(index)
                .unwrap()
                .store(keep_id, Ordering::Release);
        });
    }

    fn mini_heap_for_arena_offset(&self, arena_offset: usize) -> *mut MiniHeap {
        self.mh_index
            .get(arena_offset)
            .unwrap()
            .load(Ordering::Acquire)
            .cast()
    }

    pub fn lookup_mini_heap(&self, ptr: *mut ()) -> *mut MiniHeap {
        let offset = unsafe { ptr.offset_from(self.arena_begin as *mut ()) } as usize;
        self.mini_heap_for_arena_offset(offset)
    }

    pub fn mini_heap_for_id(&self, id: AtomicMiniHeapId<MiniHeap>) -> *mut MiniHeap<'_>{
        let mh = unsafe { self.mh_allocator.get_mut(id) };
        builtin_prefetch(mh as *mut ());
        mh
    }

    pub fn above_mesh_threshold(&self) -> bool {
        self.meshed_bitmap.in_use_count() > self.max_mesh_count as u64
    }
}
fn free_physical(fd: i32, offset: usize, size: usize) {
    assert!(size / crate::PAGE_SIZE > 0);
    assert!(size % crate::PAGE_SIZE > 0);

    //TODO:: add check for if meshing is enabled or not
    let _ = unsafe { fallocate(fd, offset, size) };
}

fn reset_span_mapping(ptr: *mut c_void, fd: i32, span: &Span) {
    unsafe {
        let _ = mmap(
            ptr,
            fd,
            span.byte_length(),
            usize::try_from(span.offset).unwrap() * PAGE_SIZE,
        )
        .unwrap();
    }
}

const fn index_size() -> usize {
    size_of::<Offset>() * ARENA_SIZE / PAGE_SIZE
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum PageType {
    Clean = 0,
    Dirty = 1,
    Meshed = 2,
    Unknown = 3,
}
