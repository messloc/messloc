use std::{
    ptr::null,
    sync::{Mutex, MutexGuard, PoisonError},
};

use crate::{PAGE_SIZE, MiniHeap};

pub struct GlobalHeap {
    data: Mutex<()>,
}

impl GlobalHeap {
    pub fn malloc(&self, bytes: usize) -> *const () {
        null()
    }

    fn pageAlignedAlloc(&self, align: usize, pages: usize) -> *const () {
        // if given a very large allocation size (e.g. (usize::MAX)-8), it is possible
        // the pages calculation overflowed.  An allocation that big is impossible
        // to satisfy anyway, so just fail early.
        if pages == 0 {
            return null();
        }

        let mut data = self.data.lock().unwrap_or_else(PoisonError::into_inner);

        let miniheap = alloc_miniheap(&mut data, -1, pages, 1, pages * PAGE_SIZE, align);

        //   d_assert(mh->isLargeAlloc());
        //   d_assert(mh->spanSize() == pageCount * kPageSize);
        //   // d_assert(mh->objectSize() == pageCount * kPageSize);

        //   void *ptr = mh->mallocAt(arenaBegin(), 0);

        return ptr;
    }
}

fn alloc_miniheap(
    lock: &mut MutexGuard<()>,
    size_class: i32,
    pages: usize,
    objects: usize,
    object_size: usize,
    align: usize,
) -> *mut MiniHeap {
    debug_assert!(pages > 0);
    // void *buf = _mhAllocator.alloc();
    // d_assert(buf != nullptr);

    // // allocate out of the arena
    // Span span{0, 0};
    // char *spanBegin = Super::pageAlloc(span, pageCount, pageAlignment);
    // d_assert(spanBegin != nullptr);
    // d_assert((reinterpret_cast<uintptr_t>(spanBegin) / kPageSize) % pageAlignment == 0);

    // MiniHeap *mh = new (buf) MiniHeap(arenaBegin(), span, objectCount, objectSize);

    // const auto miniheapID = MiniHeapID{_mhAllocator.offsetFor(buf)};
    // Super::trackMiniHeap(span, miniheapID);

    // // mesh::debug("%p (%u) created!\n", mh, GetMiniHeapID(mh));

    // _miniheapCount++;
    // _stats.mhAllocCount++;
    // _stats.mhHighWaterMark = max(_miniheapCount, _stats.mhHighWaterMark);

    // return mh;
}
