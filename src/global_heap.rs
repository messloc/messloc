use std::{
    mem::size_of,
    ptr::{null, null_mut},
    sync::{atomic::AtomicUsize, Mutex, MutexGuard, PoisonError},
};

use crate::{meshable_arena::MeshableArena, mini_heap::MiniHeap, PAGE_SIZE};

#[derive(Default)]
pub struct GlobalHeapStats {
    mesh_count: AtomicUsize,
    free_count: usize,
    alloc_count: usize,
    high_water_mark: usize,
}

#[derive(Default)]
pub struct GlobalHeapShared;

#[derive(Default)]
pub struct GlobalHeapGuarded {
    miniheap_count: usize,
    stats: GlobalHeapStats,
}

#[derive(Default)]
pub struct GlobalHeap {
    shared: GlobalHeapShared,
    guarded: Mutex<GlobalHeapGuarded>,
}
pub struct GlobalHeapLocked<'lock> {
    shared: &'lock GlobalHeapShared,
    guarded: MutexGuard<'lock, GlobalHeapGuarded>,
}

/// Returns the minimum number of pages needed to
/// hold the requested allocation
const fn page_count(bytes: usize) -> usize {
    // bytes.div_ceil(PAGE_SIZE)
    (bytes.wrapping_add(PAGE_SIZE - 1)) / PAGE_SIZE
}

impl GlobalHeap {
    /// Lock access to the GlobalHeap
    pub fn lock(&self) -> GlobalHeapLocked<'_> {
        let guarded = self.guarded.lock().unwrap_or_else(PoisonError::into_inner);
        GlobalHeapLocked {
            shared: &self.shared,
            guarded,
        }
    }

    /// Allocate a region of memory that can satisfy the requested bytes
    pub fn malloc(&self, bytes: usize) -> *const () {
        self.alloc_page_aligned(1, page_count(bytes))
    }

    /// Allocate the requested number of pages
    fn alloc_page_aligned(&self, page_align: usize, page_count: usize) -> *const () {
        // if given a very large allocation size (e.g. (usize::MAX)-8), it is possible
        // the pages calculation overflowed. An allocation that big is impossible
        // to satisfy anyway, so just fail early.
        if page_count == 0 {
            return null();
        }

        let mut lock = self.lock();

        let miniheap = lock.alloc_miniheap(page_count, 1, page_count * PAGE_SIZE, page_align);

        //   d_assert(mh->isLargeAlloc());
        //   d_assert(mh->spanSize() == pageCount * kPageSize);
        //   // d_assert(mh->objectSize() == pageCount * kPageSize);

        unsafe { (*miniheap).malloc_at(lock.guarded.arena.arena_begin, 0) }
    }
}

impl GlobalHeapLocked<'_> {
    fn alloc_miniheap(
        &mut self,
        page_count: usize,
        object_count: usize,
        object_size: usize,
        page_align: usize,
    ) -> *mut MiniHeap {
        debug_assert!(page_count > 0, "should allocate at least 1 page");

        let buf = unsafe { self.guarded.arena.mh_allocator.alloc() };
        debug_assert_ne!(buf, null_mut());

        // allocate out of the arena
        let (span, span_begin) = self.guarded.arena.page_alloc(page_count, page_align);

        debug_assert_ne!(span_begin, null_mut(), "arena allocation failed");
        debug_assert_eq!(
            span_begin
                .cast::<[u8; PAGE_SIZE]>()
                .align_offset(page_align),
            0,
            "arena allocation unaligned"
        );

        debug_assert!(size_of::<MiniHeap>() <= 64);
        let mh = buf.cast();
        unsafe { MiniHeap::new_inplace(mh, span, object_count, object_size) }

        let id = unsafe { self.guarded.arena.mh_allocator.offset_for(buf) };
        unsafe { self.guarded.arena.track_miniheap(span, id) };

        // // mesh::debug("%p (%u) created!\n", mh, GetMiniHeapID(mh));

        self.guarded.miniheap_count += 1;
        self.guarded.stats.alloc_count += 1;
        self.guarded.stats.high_water_mark = self
            .guarded
            .miniheap_count
            .max(self.guarded.stats.high_water_mark);

        mh
    }
}
