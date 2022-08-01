use libc::c_void;

use crate::arena_fs::open_shm_span_file;
use crate::runtime::Runtime;
use crate::span::{Length, Offset, Span, SpanList};
use crate::{
    cheap_heap::CheapHeap,
    mini_heap::{AtomicMiniHeapId, MiniHeap},
    one_way_mmap_heap::Heap,
    ARENA_SIZE, DIRTY_PAGE_THRESHOLD, MIN_ARENA_EXPANSION, PAGE_SIZE, SPAN_CLASS_COUNT,
};
use std::mem::size_of;
use std::{
    path::PathBuf,
    pointer::offset_from,
    ptr::null_mut,
    sync::atomic::Ordering,
    sync::{Arc, Mutex},
};

use crate::{utils::*, MAP_SHARED};
pub type Page = [u8; PAGE_SIZE];

pub struct MeshableArena {
    pub runtime: Arc<Mutex<Runtime>>,
    pub(crate) arena_begin: *mut Page,
    fd: i32,
    /// offset in pages
    end: Offset,
    dirty: SpanList,
    clean: SpanList,
    freed_spans: SpanList,
    mh_index: AtomicMiniHeapId<CheapHeap<64, { ARENA_SIZE / PAGE_SIZE }>>,
    pub(crate) mh_allocator: CheapHeap<64, { ARENA_SIZE / PAGE_SIZE }>,
    meshed_bitmap: MeshedBitmap,
    fork_pipe: [i32; 2],
    span_dir: PathBuf,
    meshed_page_count_hwm: usize,
}

impl MeshableArena {
    pub fn init() -> MeshableArena {
        // TODO: check if meshing enabled
        //TODO: initialise stuff from the constructor

        let fd = open_shm_span_file(ARENA_SIZE);

        let mh_allocator = CheapHeap::new();
        let arena_begin =
            unsafe { mh_allocator.map(ARENA_SIZE, MAP_SHARED, fd).inner() as *mut Page };
        let mh_index = unsafe { mh_allocator.malloc(index_size()) };

        MeshableArena {
            runtime: Arc::new(Mutex::new(Runtime::default())),
            arena_begin,
            fd,
            //TODO:: find initial end value
            end: Offset::default(),
            dirty: SpanList::default(),
            clean: SpanList::default(),
            mh_allocator,
            mh_index,
            meshed_bitmap: MeshedBitmap::default(),
            freed_spans: SpanList::default(),
            fork_pipe: [-1, -1],
            span_dir: PathBuf::default(),
            meshed_page_count_hwm: 0,
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

        self.clean.inner()[expansion.class() as usize].push(expansion)

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
        let spans = &mut free_spans.get(span_class as usize).unwrap();
        if spans.is_empty() {
            return None;
        }

        // let old_len = spans.len();
        let end = spans.len() - 1;

        if span_class == SPAN_CLASS_COUNT - 1 && spans[end].length < page_count {
            // the final span class contains (and is the only class to
            // contain) variable-size spans, so we need to make sure we
            // search through all candidates in this case.
            for j in 0..end {
                if spans[j].length >= page_count {
                    spans.swap(j, end);
                    break;
                }
            }

            // check that we found something in the above loop. this would be
            // our last loop iteration anyway
            if spans[end].length < page_count {
                return None;
            }
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
            free_spans[rest.class() as usize].push(rest);
        }
        debug_assert_eq!(span.length, page_count);

        Some(span)
    }

    pub unsafe fn track_miniheap(&mut self, span: Span, id: u32) {
        for i in 0..span.length {
            self.set_index((span.offset + i) as usize, id)
        }
    }

    pub unsafe fn set_index(&mut self, offset: usize, id: u32) {
        (&*self.mh_index.add(offset)).store(id, Ordering::Release);
    }

    fn scavenge(&self, force: bool) {
        if force && self.dirty.len() < DIRTY_PAGE_THRESHOLD {
            let mut bitmap = Bitmap::new();
            bitmap.invert();

            self.freed_spans.iter().for_each(|span_list| {
                span_list.iter().enumerate().for_each(|(key, span)| {
                    self.meshed_bitmap.unset(span.offset as usize + key);
                    (0..=span.length).for_each(|k| bitmap.try_set(span.offset + k));
                    self.meshed_bitmap.reset_span_mapping(span);
                })
            });

            self.freed_spans.clear();

            let page_count = self.mashed_bitmap.in_use_count();
            if page_count > self.meshed_page_count_hwm {
                self.meshed_page_count_hwm = page_count;
            }

            for_each_free(self.dirty, |span| {
                let ptr = unsafe { self.arena_begin.add(span.offset) };
                let size = span.byte_length();
                self.free_physical(ptr, span.offset as usize, size);
                (0..=span.length).for_each(|k| bitmap.try_set(span.offset + k));
            });

            self.dirty.clear();
            self.clean.clear();

            self.coalesce(bitmap);
        }
    }

    fn coalesce(&self, bitmap: Bitmap) {
        let current = Span::default();
        for i in bitmap.iter() {
            if i == current.offset + current.length {
                current.length += 1;
                continue;
            }

            if !current.is_empty() {
                self.clean
                    .inner()
                    .get_mut(current.class() as usize)
                    .unwrap()
                    .push(current);
            }

            current = Span::new(i, 1);
        }
    }

    fn free_physical(&self, ptr: *mut [u8], offset: usize, size: usize) {
        let ptr = unsafe { self.arena_begin.add(offset) };

        assert!(size / crate::PAGE_SIZE > 0);
        assert!(size % crate::PAGE_SIZE > 0);

        //TODO:: add check for if meshing is enabled or not
        let _ = unsafe { fallocate(self.fd, offset, size) };
    }

    fn partial_scavenge(&self) {
        self.dirty.for_each_free(|span| {
            let ptr = unsafe { self.arena_begin.add(span.offset as usize) };
            let size = span.byte_length();
            unsafe { madvise(ptr as *mut c_void, size) };
            self.free_physical(ptr, span.offset as usize, size);
            self.clean
                .get_mut(span.class() as usize)
                .unwrap()
                .push(*span);
        });

        self.dirty.clear();
    }

    fn begin_mesh(&self, remove: *mut [u8], size: usize) {
        let _ = unsafe { mprotect_read(remove as *mut c_void, size).unwrap() };
    }

    fn finalise_mesh(&self, keep: *mut (), remove: *mut (), size: usize) {
        let keep_offset = unsafe { keep.offset_from(self.arena_begin as *mut ()) };
        let remove_offset = unsafe { remove.offset_from(self.arena_begin as *mut ()) };
        let page_count = size / PAGE_SIZE;
        let keep_id = self
            .mh_index
            .get(usize::try_from(keep_offset).unwrap())
            .unwrap()
            .load(Ordering::Acquire);
        self.store_indices(keep_id, remove_offset, page_count);
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

    fn store_indices(&self, keep_id: u32, offset: isize, page_count: usize) {
        (0..page_count).for_each(|index| {
            self.mh_index
                .get(index)
                .unwrap()
                .store(keep_id, Ordering::Release);
        });
    }

    fn mini_heap_for_arena_offset(&self, arena_offset: usize) -> Option<MiniHeap> {
        let mh_offset = self
            .mh_index
            .get(arena_offset)
            .unwrap()
            .load(Ordering::Acquire);
        self.mh_allocator.get(mh_offset)
    }
}

const fn index_size() -> usize {
    size_of::<Offset>() * ARENA_SIZE / PAGE_SIZE
}

#[derive(PartialEq, Debug, Clone, Copy)]
enum PageType {
    Clean = 0,
    Dirty = 1,
    Meshed = 2,
    Unknown = 3,
}
