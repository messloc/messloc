use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::{
    cheap_heap::DynCheapHeap,
    one_way_mmap_heap::{Heap, OneWayMmapHeap},
};
pub struct InternalHeap;
lazy_static::lazy_static! {
    static ref HEAP: Mutex<PartitionedHeap> = Mutex::new(PartitionedHeap::new());
}

impl InternalHeap {
    fn get(&mut self) -> MutexGuard<'_, PartitionedHeap> {
        HEAP.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Heap for InternalHeap {
    unsafe fn map(&mut self, size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut () {
        self.get().map(size, flags, fd)
    }

    unsafe fn malloc(&mut self, size: usize) -> *mut () {
        self.get().malloc(size)
    }

    unsafe fn get_size(&mut self, p: *mut ()) -> usize {
        self.get().get_size(p)
    }

    unsafe fn free(&mut self, p: *mut ()) {
        self.get().free(p)
    }
}

const PARTITIONED_HEAP_BINS: usize = 16;
const PARTITIONED_HEAP_ARENA_SIZE: usize = 512 * 1024 * 1024; // 512 MB
const PARTITIONED_HEAP_SIZE_PER: usize = PARTITIONED_HEAP_ARENA_SIZE / PARTITIONED_HEAP_BINS;

pub struct PartitionedHeap {
    small_arena: *mut u8,
    small_heaps: [DynCheapHeap; PARTITIONED_HEAP_BINS],
    big_heap: (),
}
unsafe impl Send for PartitionedHeap {}

impl PartitionedHeap {
    fn new() -> Self {
        let small_arena =
            unsafe { OneWayMmapHeap.malloc(PARTITIONED_HEAP_ARENA_SIZE) }.cast::<u8>();
        let freelist = unsafe { OneWayMmapHeap.malloc(PARTITIONED_HEAP_ARENA_SIZE) }.cast::<u8>();
        let mut i = 0;
        let small_heaps = [(); PARTITIONED_HEAP_BINS].map(|_| {
            let arena = unsafe { small_arena.add(i * PARTITIONED_HEAP_SIZE_PER) };
            let freelist = unsafe { freelist.add(i * PARTITIONED_HEAP_SIZE_PER) };

            let alloc_size = 8 << i;
            let max_count = PARTITIONED_HEAP_SIZE_PER / alloc_size;

            i += 1;
            DynCheapHeap::new(alloc_size, max_count, arena, freelist.cast())
        });

        Self {
            small_arena,
            small_heaps,
            big_heap: (),
        }
    }
}

impl crate::one_way_mmap_heap::Heap for PartitionedHeap {
    unsafe fn map(&mut self, size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut () {
        todo!()
    }

    unsafe fn malloc(&mut self, size: usize) -> *mut () {
        todo!()
    }

    unsafe fn get_size(&mut self, ptr: *mut ()) -> usize {
        todo!()
    }

    unsafe fn free(&mut self, ptr: *mut ()) {
        todo!()
    }
}
