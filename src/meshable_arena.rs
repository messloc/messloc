use crate::arena_fs::open_shm_span_file;
use crate::bitmap::{Bitmap, BitmapBase, RelaxedBitmapBase};
use crate::comparatomic::Comparatomic;
use crate::one_way_mmap_heap::OneWayMmapHeap;
use crate::span::{Length, Offset, Span, SpanList};
use crate::{
    cheap_heap::CheapHeap,
    fake_std::dynarray::DynArray,
    flags::Flags,
    mini_heap::{MiniHeap, MiniHeapId},
    one_way_mmap_heap::Heap,
    ARENA_SIZE, DEFAULT_MAX_MESH_COUNT, DIRTY_PAGE_THRESHOLD, MIN_ARENA_EXPANSION, PAGE_SIZE,
    SPAN_CLASS_COUNT,
};
use crate::{for_each_meshed, NUM_BINS};
use core::mem::{size_of, MaybeUninit};
use core::ptr::addr_of_mut;
use core::{
    ptr::null_mut,
    sync::atomic::{AtomicPtr, Ordering},
};
use spin::mutex::Mutex;

use crate::{
    utils::{fallocate, madvise, mmap, mprotect_read},
    MAP_SHARED,
};
use libc::c_void;
pub type Page = [u8; PAGE_SIZE];

pub struct MeshableArena {
    pub(crate) arena_begin: *mut (),
    fd: i32,
    /// offset in pages
    end: Offset,
    dirty: *mut SpanList<256, SPAN_CLASS_COUNT>,
    clean: *mut SpanList<256, SPAN_CLASS_COUNT>,
    pub freed_spans: *mut SpanList<256, SPAN_CLASS_COUNT>,
    pub mini_heaps: DynArray<MiniHeap, NUM_BINS>,
    pub(crate) mh_allocator: *mut (),
    meshed_bitmap: *mut Bitmap<RelaxedBitmapBase<{ ARENA_SIZE / PAGE_SIZE }>>,
    fork_pipe: [i32; 2],
    mini_heap_count: usize,
    meshed_page_count_hwm: u64,
    max_mesh_count: usize,
}

unsafe impl Sync for MeshableArena {}
unsafe impl Send for MeshableArena {}

impl MeshableArena {
    pub fn init() -> Self {
        // TODO: check if meshing enabled
        let fd = open_shm_span_file(ARENA_SIZE);
        let mut mh_allocator: CheapHeap<8, { ARENA_SIZE / PAGE_SIZE }> = CheapHeap::new();
        Self {
            arena_begin: null_mut(),
            fd,
            end: Offset::default(),
            dirty: SpanList::alloc_new(),
            clean: SpanList::alloc_new(),
            freed_spans: SpanList::alloc_new(),
            mh_allocator: &mh_allocator as *const _ as *mut _,
            mini_heaps: DynArray::<MiniHeap, NUM_BINS>::create(),
            meshed_bitmap: Bitmap::alloc_new(),
            fork_pipe: [-1, -1],
            mini_heap_count: 0,
            meshed_page_count_hwm: 0,
            max_mesh_count: DEFAULT_MAX_MESH_COUNT,
        }
    }
    pub fn page_alloc(&mut self, page_count: usize, page_align: usize) -> (Span, *mut Page) {
        if page_count == 0 {
            return (Span::default(), null_mut());
        }

        debug_assert!(page_count > 0);

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
        let ptr = unsafe { self.arena_begin.cast::<MiniHeap>().add(span.offset) };

        //     if (kAdviseDump) {
        //       madvise(ptr, pageCount * kPageSize, MADV_DODUMP);
        //     }

        (span, ptr.cast())
    }

    fn reserve_pages(&mut self, page_count: usize, page_align: usize) -> Span {
        debug_assert!(page_count > 0);
        let (result, flags) = if let Some((span, flags)) = self.find_pages(page_count) {
            (span, flags)
        } else {
            self.expand_arena(page_count);
            self.find_pages(page_count).unwrap() // unchecked?
        };

        debug_assert!(!result.is_empty());
        debug_assert_ne!(flags, PageType::Unknown);

        let ptr = unsafe { self.arena_begin.cast::<MiniHeap>().add(result.offset) };
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
            length: page_count,
        };
        self.end += page_count;

        //   if (unlikely(_end >= kArenaSize / kPageSize)) {
        //     debug("Mesh: arena exhausted: current arena size is %.1f GB; recompile with larger arena size.",
        //           kArenaSize / 1024.0 / 1024.0 / 1024.0);
        //     abort();
        //   }

        let clean = unsafe { self.clean.as_mut().unwrap() };

        unsafe { clean.inner_mut()[expansion.class()].push(expansion) };

        //   _clean[expansion.spanClass()].push_back(expansion);
    }

    fn find_pages(&mut self, page_count: usize) -> Option<(Span, PageType)> {
        // Search through all dirty spans first.  We don't worry about
        // fragmenting dirty pages, as being able to reuse dirty pages means
        // we don't increase RSS.
        let span = Span {
            offset: 0,
            length: page_count,
        };

        for span_class in span.class()..SPAN_CLASS_COUNT {
            if let Some(span) = Self::find_pages_inner(self.dirty, span_class, page_count) {
                return Some((span, PageType::Dirty));
            }
        }

        // if no dirty pages are available, search clean pages.  An allocated
        // clean page (once it is written to) means an increased RSS.
        for span_class in span.class()..SPAN_CLASS_COUNT {
            if let Some(span) = Self::find_pages_inner(self.clean, span_class, page_count) {
                return Some((span, PageType::Clean));
            }
        }

        None
    }

    fn find_pages_inner<const N: usize>(
        free_spans: *mut SpanList<N, SPAN_CLASS_COUNT>,
        span_class: usize,
        page_count: usize,
    ) -> Option<Span> {
        let mut spans = unsafe { free_spans.as_mut().unwrap().get_mut(span_class).unwrap() };
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
            let free_spans = unsafe { free_spans.as_mut().unwrap() };
            free_spans.get_mut(rest.class()).unwrap().push(rest);
        }
        debug_assert_eq!(span.length, page_count);

        Some(span)
    }

    ///# Safety
    /// Unsafe
    ///
    pub unsafe fn generate_mini_heap(&mut self, alloc: *mut (), bytes: usize) -> *mut MiniHeap {
        let mini_heaps = self.mini_heaps.as_mut_slice().as_mut().unwrap();
        let empty = mini_heaps.iter().position(|x| x.is_null());
        let mut new_heap = MiniHeap::new(alloc, Span::default(), bytes);
        match empty {
            Some(pos) => {
                mini_heaps[pos].write(new_heap);
                self.mini_heaps.inner().add(pos)
            }

            None => {
                core::mem::replace(&mut mini_heaps[0], &mut new_heap);
                //mini_heaps[0].write(new_heap);
                {}
                self.mini_heaps.get(0)
            }
        }
    }
    ///# Safety
    /// Unsafe
    pub unsafe fn get_mini_heap(&self, ptr: *mut ()) -> Option<*mut MiniHeap> {
        let mini_heaps = self.mini_heaps.as_slice();

        mini_heaps
            .as_ref()
            .unwrap()
            .iter()
            .take_while(|x| !x.is_null())
            .find(|&&mh| mh.as_mut().unwrap().arena_begin == ptr.cast())
            .map(|x| x as *const _ as *mut MiniHeap)
    }

    pub fn scavenge(&mut self, force: bool) {
        let (clean, dirty, freed_spans) = unsafe {
            (
                self.clean.as_mut().unwrap(),
                self.dirty.as_mut().unwrap(),
                self.freed_spans.as_mut().unwrap(),
            )
        };

        let meshed_bitmap = unsafe { self.meshed_bitmap.as_mut().unwrap() };
        if force && dirty.len() < DIRTY_PAGE_THRESHOLD {
            let mut bitmap: Bitmap<RelaxedBitmapBase<{ ARENA_SIZE / PAGE_SIZE }>> =
                Bitmap::default();

            bitmap.invert();

            freed_spans.inner().iter().for_each(|span_list| {
                span_list.iter().enumerate().for_each(|(key, span)| {
                    meshed_bitmap.unset(span.offset + key);
                    (0..=span.length).for_each(|k| {
                        bitmap.try_to_set(span.offset + k);
                    });
                    let ptr = unsafe { self.arena_begin.cast::<MiniHeap>().add(span.offset) };
                    reset_span_mapping(ptr as *mut c_void, self.fd, span);
                });
            });

            freed_spans.clear();

            let page_count = meshed_bitmap.in_use_count();
            if page_count > self.meshed_page_count_hwm {
                self.meshed_page_count_hwm = page_count;
            }

            dirty.for_each_free(|span| {
                let size = span.byte_length();
                free_physical(self.fd, span.offset, size);
                (0..=span.length).for_each(|k| {
                    bitmap.try_to_set(span.offset + k);
                });
            });

            dirty.clear();
            clean.clear();

            self.coalesce(&bitmap);
        }
    }

    fn coalesce<const N: usize>(&mut self, bitmap: &Bitmap<RelaxedBitmapBase<N>>) {
        let mut current = Span::default();
        for i in bitmap.inner().bits().iter() {
            if *i == (current.offset + current.length) as u64 {
                current.length += 1;
                continue;
            }

            if !current.is_empty() {
                unsafe {
                    (*self.clean)
                        .inner_mut()
                        .get_mut(current.class())
                        .unwrap()
                        .push(current);
                }
            }

            current = Span::new(usize::try_from(*i).unwrap(), 1);
        }
    }

    fn partial_scavenge(&mut self) {
        unsafe {
            (*self.dirty).for_each_free(|span| {
                let ptr = unsafe { self.arena_begin.cast::<MiniHeap>().add(span.offset) };
                let size = span.byte_length();
                unsafe { madvise(ptr as *mut libc::c_void, size) }.unwrap();
                free_physical(self.fd, span.offset, size);
                (*self.clean)
                    .get_mut(span.class())
                    .unwrap()
                    .push(span.clone());
            });
        }

        unsafe { (*self.dirty).clear() };
    }

    #[allow(clippy::unused_self)]
    ///# Safety
    /// Unsafe
    pub unsafe fn begin_mesh(&self, remove: *mut c_void, size: usize) {
        unsafe { mprotect_read(remove, size).unwrap() };
    }

    fn finalise_mesh(&mut self, keep: *mut (), remove: *mut (), size: usize) {
        let keep_offset = unsafe { keep.offset_from(self.arena_begin as *mut ()) };
        let remove_offset = unsafe { remove.offset_from(self.arena_begin as *mut ()) };
        let page_count = size / PAGE_SIZE;
        let removed_span = Span::new(usize::try_from(remove_offset).unwrap(), page_count);
        unsafe {
            self.meshed_bitmap
                .as_mut()
                .unwrap()
                .track_meshed(removed_span);
            mmap(
                remove as *mut c_void,
                self.fd,
                size,
                usize::try_from(keep_offset).unwrap() * PAGE_SIZE,
            )
            .unwrap()
        };
    }
    ///# Safety
    /// Unsafe
    pub unsafe fn lookup_mini_heap(&self, ptr: *mut ()) -> *mut MiniHeap {
        let offset = unsafe { ptr.offset_from(self.arena_begin as *mut ()) } as usize;

        let mh = unsafe {
            self.mh_allocator
                .cast::<MiniHeapId>()
                .add(offset)
                .as_mut()
                .unwrap()
        };

        if let MiniHeapId::HeapPointer(heap) = mh {
            *heap
        } else {
            unreachable!()
        }
    }

    pub fn above_mesh_threshold(&self) -> bool {
        unsafe { (*self.meshed_bitmap).in_use_count() > self.max_mesh_count as u64 }
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
        let _ = mmap(ptr, fd, span.byte_length(), span.offset * PAGE_SIZE).unwrap();
    }
}

pub const fn index_size() -> usize {
    size_of::<Offset>() * ARENA_SIZE / PAGE_SIZE
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum PageType {
    Clean = 0,
    Dirty = 1,
    Meshed = 2,
    Unknown = 3,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_mini_heap() {
        let mut arena = MeshableArena::init();
        unsafe { arena.generate_mini_heap(null_mut(), 0) };
        unsafe { arena.generate_mini_heap(null_mut(), 0) };
    }
}
