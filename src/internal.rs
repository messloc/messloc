use crate::{
    cheap_heap::DynCheapHeap,
    mmap_heap::MmapHeap,
    one_way_mmap_heap::{Heap, OneWayMmapHeap},
};
use std::sync::{Mutex, MutexGuard, PoisonError};
pub struct InternalHeap;

impl InternalHeap {
    fn get(&mut self) -> MutexGuard<'_, PartitionedHeap> {
        use lazy_static::lazy::Lazy;
        fn init() -> Mutex<PartitionedHeap> {
            Mutex::new(PartitionedHeap::new())
        }
        static LAZY: Lazy<Mutex<PartitionedHeap>> = Lazy::INIT;
        LAZY.get(init)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Heap for InternalHeap {
    unsafe fn map(&mut self, size: usize, flags: libc::c_int, fd: libc::int) -> *mut () {
        todo!()
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
    big_heap: MmapHeap,
}
unsafe impl Send for PartitionedHeap {}

impl PartitionedHeap {
    pub fn new() -> Self {
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
            big_heap: MmapHeap::default(),
        }
    }

    unsafe fn get_size_class(&self, ptr: *mut ()) -> usize {
        let offset = ptr.cast::<u8>().offset_from(self.small_arena) as usize;
        let size_class = offset / PARTITIONED_HEAP_SIZE_PER;
        debug_assert!((0..PARTITIONED_HEAP_BINS).contains(&size_class));
        size_class
    }

    unsafe fn contains(&self, ptr: *mut ()) -> bool {
        (self.small_arena..self.small_arena.add(PARTITIONED_HEAP_ARENA_SIZE)).contains(&ptr.cast())
    }
}

pub const fn log2(x: usize) -> u32 {
    usize::BITS - 1 - x.leading_zeros()
}

impl crate::one_way_mmap_heap::Heap for PartitionedHeap {
    unsafe fn map(&mut self, size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut () {
        todo!()
    }

    unsafe fn malloc(&mut self, size: usize) -> *mut () {
        let size_class = (log2(size.max(8)) - 3) as usize;

        if size_class >= PARTITIONED_HEAP_BINS {
            self.big_heap.malloc(size)
        } else {
            self.small_heaps[size_class].alloc().cast()
        }
    }

    unsafe fn get_size(&mut self, ptr: *mut ()) -> usize {
        if !self.contains(ptr) {
            self.big_heap.get_size(ptr)
        } else {
            let size_class = self.get_size_class(ptr);
            8 << size_class
        }
    }

    unsafe fn free(&mut self, ptr: *mut ()) {
        if !self.contains(ptr) {
            self.big_heap.free(ptr)
        } else {
            let size_class = self.get_size_class(ptr);
            self.small_heaps[size_class].free(ptr);
        }
    }
}
