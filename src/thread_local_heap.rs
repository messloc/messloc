use crate::class_array::CLASS_ARRAY;
use crate::mini_heap::MiniHeap;
use crate::shuffle_vector::{self, ShuffleVector};
use crate::{global_heap::GlobalHeap, one_way_mmap_heap::Heap};
use crate::{
    MAX_SHUFFLE_VECTOR_LENGTH, MAX_SIZE, MAX_SMALL_SIZE, MIN_OBJECT_SIZE, NUM_BINS, PAGE_SIZE,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
pub struct ThreadLocalHeap {
    shuffle_vector: [ShuffleVector<MAX_SHUFFLE_VECTOR_LENGTH>; NUM_BINS],
    global_heap: GlobalHeap,
    current: u64,
}

impl ThreadLocalHeap {
    pub fn current(&self) -> u64 {
        self.current
    }

    pub fn memalign(&mut self, alignment: usize, size: usize) -> Option<*mut ()> {
        if alignment != 0 && alignment & (alignment - 1) == 0 {
            let size = size.max(8);
            if alignment <= std::mem::size_of::<u64>() {
                let ptr = unsafe { self.malloc(size) };
                Some(ptr)
            } else if let Some(sc) = SizeMap.get_size_class(size) {
                let size_class_bytes = SizeMap.bytes_size_for_class(sc);
                if size_class_bytes <= PAGE_SIZE
                    && alignment <= size_class_bytes
                    && size_class_bytes % alignment == 0
                {
                    let ptr = unsafe { self.malloc(size) };
                    Some(ptr)
                } else {
                    None
                }
            } else {
                let page_alignment = (alignment + PAGE_SIZE - 1) / PAGE_SIZE;
                Some(self.global_heap.page_aligned_alloc(page_alignment, size))
            }
        } else {
            None
        }
    }

    fn alloc_slow_path(&mut self, size_class: usize) {
        self.small_alloc_global_refill(size_class);
    }

    fn small_alloc_global_refill(&mut self, size_class: usize) {
        let size_max = SizeMap.bytes_size_for_class(size_class);
        let current = self.current;

        self.global_heap.small_alloc_mini_heaps(
            size_class,
            size_max,
            &self.shuffle_vector,
            current,
        );
    }

    pub fn release_all(&mut self) {
        self.shuffle_vector.iter_mut().for_each(|mut sv| {
            sv.refill_mini_heaps();
            sv.mini_heaps.iter().for_each(|mh| {
                self.global_heap.release_mini_heap_locked(*mh);
            });
        });
    }

    pub unsafe fn malloc(&mut self, size: usize) -> *mut () {
        if let Some(size_class) = SizeMap.get_size_class(size) {
            if self.shuffle_vector[size_class].is_exhausted_and_no_refill() {
                self.alloc_slow_path(size_class);
            }

            self.shuffle_vector[size_class].malloc() as *mut ()
        } else {
            self.global_heap.malloc(size) as *mut ()
        }
    }
}

impl Heap for ThreadLocalHeap {
    type PointerType = *mut ();
    type MallocType = *mut ();

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

struct SizeMap;

impl SizeMap {
    pub fn get_size_class(&self, size: usize) -> Option<usize> {
        let idx = self.class_index_maybe(size)?;
        Some(CLASS_ARRAY[idx] as usize)
    }

    pub fn class_index_maybe(&self, size: usize) -> Option<usize> {
        // this is overlapping but allowed because it currently is the nicest way
        // to write `MAX_SMALL_SIZE+1..MAX_SIZE`
        #[allow(clippy::match_overlapping_arm)]
        match size {
            0..=MAX_SMALL_SIZE => Some((size + 7) >> 3),
            ..=MAX_SIZE => Some((size + 127 + (120 << 7)) >> 7),
            _ => None,
        }
    }

    pub const fn bytes_size_for_class(&self, size: usize) -> usize {
        1 << (size + crate::utils::stlog(MIN_OBJECT_SIZE))
    }
}
