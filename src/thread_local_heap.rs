use crate::class_array::CLASS_ARRAY;
use crate::{one_way_mmap_heap::Heap, global_heap::GlobalHeap};
use crate::shuffle_vector::{ShuffleVector, self};
use crate::{NUM_BINS, MAX_SMALL_SIZE, MAX_SIZE, PAGE_SIZE, MIN_OBJECT_SIZE, MAX_SHUFFLE_VECTOR_LENGTH};
use std::sync::{Arc, Mutex};
pub struct ThreadLocalHeap<'a>{
    shuffle_vector: [ShuffleVector<'a, MAX_SHUFFLE_VECTOR_LENGTH>; NUM_BINS],
    global_heap: Arc<Mutex<GlobalHeap<'a>>>,
    current: u64,
}

impl ThreadLocalHeap<'_>{

    pub fn current(&self) -> u64 {
        self.current
    }

    pub fn memalign(&self, alignment: usize, size: usize) -> Option<*mut ()> {
      if alignment != 0 && alignment & alignment - 1 == 0 {
        let size = size.max(8);
        if alignment <= std::mem::size_of::<u64>() {
        let ptr = unsafe { self.malloc(size) };
        return Some(ptr);
      } else if let Some(sc) = SizeMap.get_size_class(size) {
          let size_class_bytes = SizeMap.bytes_size_for_class(sc);
          if size_class_bytes <= PAGE_SIZE && alignment <= size_class_bytes && size_class_bytes % alignment == 0 {
            let ptr = unsafe { self.malloc(size) };
            return Some(ptr);
          } else {
              return None;
          }
      } else {
        let page_alignment = (alignment + PAGE_SIZE - 1) / PAGE_SIZE;
        Some((*self.global_heap.lock().unwrap()).page_aligned_alloc(page_alignment, size))
      }
} else {
    None
}
}

fn alloc_slow_path(&mut self, size_class: usize) -> *mut () {
    let vector = self.shuffle_vector[size_class];
    if vector.local_refill() {
        vector.malloc();
    }

    self.small_alloc_global_refill(&vector, size_class)
}

fn small_alloc_global_refill(&self, vector: &ShuffleVector<'_, MAX_SHUFFLE_VECTOR_LENGTH>, size_class: usize) -> *mut () {
    let size_max = SizeMap.bytes_size_for_class(size_class);
    (*self.global_heap.lock().unwrap()).small_alloc_mini_heaps(size_class, size_max, vector.mini_heaps_mut(), self.current());

    vector.re_init();
    assert!(!vector.is_exhausted());
    vector.malloc()
    
}

pub fn release_all(&mut self) {
    self.shuffle_vector.iter().for_each(|sv| {
        sv.refill_mini_heaps();
        let heap = (*self.global_heap.lock().unwrap());
        sv.mini_heaps_mut().iter_mut().for_each(|mh| {
            heap.release_mini_heap_locked(mh)
        });

    });
}
}

impl Heap for ThreadLocalHeap<'_> {
    type PointerType = *mut ();
    type MallocType = *mut ();

    unsafe fn map(&mut self, size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut () {
        todo!()
    }

    unsafe fn malloc(&mut self, size: usize) -> *mut () {
       if let Some(size_class) = SizeMap.get_size_class(size) {
            let vector = self.shuffle_vector[size_class];

            if vector.is_exhausted() {
                self.alloc_slow_path(size_class)
            } else {
                vector.malloc()
            }
       } else {
           self.global_heap.lock().unwrap().malloc(size) as *mut ()
       }
            
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
