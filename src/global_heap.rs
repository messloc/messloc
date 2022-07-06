use std::{
    ptr::{null, null_mut},
    sync::{Mutex, MutexGuard, PoisonError},
};

use crate::{
    meshable_arena::{MeshableArena, Span},
    mini_heap::MiniHeap, consts::PAGE_SIZE,
};

pub struct GlobalHeapShared;
pub struct GlobalHeapGuarded {
    arena: MeshableArena,
}
pub struct GlobalHeap {
    shared: GlobalHeapShared,
    guarded: Mutex<GlobalHeapGuarded>,
}
pub struct GlobalHeapLocked<'a> {
    shared: &'a GlobalHeapShared,
    guarded: MutexGuard<'a, GlobalHeapGuarded>,
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

        let miniheap = lock.alloc_miniheap(-1, page_count, 1, page_count * PAGE_SIZE, page_align);

        //   d_assert(mh->isLargeAlloc());
        //   d_assert(mh->spanSize() == pageCount * kPageSize);
        //   // d_assert(mh->objectSize() == pageCount * kPageSize);

        //   void *ptr = mh->mallocAt(arenaBegin(), 0);

        null()
    }
}

impl GlobalHeapLocked<'_> {
    fn alloc_miniheap(
        &mut self,
        size_class: i32,
        page_count: usize,
        object_count: usize,
        object_size: usize,
        page_align: usize,
    ) -> *mut MiniHeap {
        debug_assert!(page_count > 0, "should allocate at least 1 page");

        // void *buf = _mhAllocator.alloc();
        // d_assert(buf != nullptr);

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

        // MiniHeap *mh = new (buf) MiniHeap(arenaBegin(), span, objectCount, objectSize);

        // const auto miniheapID = MiniHeapID{_mhAllocator.offsetFor(buf)};
        // Super::trackMiniHeap(span, miniheapID);

        // // mesh::debug("%p (%u) created!\n", mh, GetMiniHeapID(mh));

        // _miniheapCount++;
        // _stats.mhAllocCount++;
        // _stats.mhHighWaterMark = max(_miniheapCount, _stats.mhHighWaterMark);

        // return mh;

        null_mut()
    }
}
