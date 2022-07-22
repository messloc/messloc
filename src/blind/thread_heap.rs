use std::alloc::Layout;
use std::ptr::NonNull;

use rand::Rng;

use super::MiniHeap;

use super::SIZE_CLASSES;
use super::shuffle_vec::ShuffleVector;
use super::MAX_ALLOCATIONS_PER_SPAN;
use super::SIZE_CLASS_COUNT;
use super::span::SpanAllocator;

pub struct ThreadHeap<R, H> {
    rng: R,
    shuffle_vec: [ShuffleVector<MAX_ALLOCATIONS_PER_SPAN>; SIZE_CLASS_COUNT],
    mini_heaps: [Option<MiniHeap<H>>; SIZE_CLASS_COUNT],
}

pub struct MiniHeapRequest {
    index: usize,
    size_class: u16,
}

impl MiniHeapRequest {
    pub fn size_class(&self) -> u16 {
        self.size_class
    }
    
    pub fn size_class_index(&self) -> usize {
        self.index
    }
}

impl<R: Rng, H> ThreadHeap<R, H> {
    pub fn new(rng: R) -> Self {
        Self {
            rng,
            shuffle_vec: SIZE_CLASSES.map(|_| ShuffleVector::new()),
            mini_heaps: SIZE_CLASSES.map(|_| None),
        }
    }

    pub unsafe fn alloc(&mut self, layout: Layout) -> Result<NonNull<[u8]>, MiniHeapRequest> {
        // find the size class
        if let Some(index) = SIZE_CLASSES.iter().position(|&size_class| size_class as usize >= layout.size()) {
            if let Some(mini_heap) = &mut self.mini_heaps[index] {
                if let Some(offset) = self.shuffle_vec[index].pop() {
                    return Ok(mini_heap.alloc(offset));
                }
            }

            // no mini heap of this size, we need one allocated
            Err(MiniHeapRequest { index, size_class: SIZE_CLASSES[index] })
        } else {
            panic!("layout larger than all size classes {:?}", layout);
        }
    }

    pub fn replace_mini_heap(&mut self, request: MiniHeapRequest, mini_heap: MiniHeap<H>) -> Option<MiniHeap<H>> {
        self.shuffle_vec[request.index].fill(&mut self.rng, mini_heap.free_iter());
        std::mem::replace(&mut self.mini_heaps[request.index], Some(mini_heap))
    }
}

impl<R, H> ThreadHeap<R, H> {
    pub unsafe fn drop_heaps<S: SpanAllocator<Handle = H>>(&mut self, span_alloc: &mut S) -> Result<(), S::DeallocError> {
        for mini_heap in self.mini_heaps.iter_mut() {
            if let Some(mut mini_heap) = mini_heap.take() {
                span_alloc.deallocate_span(mini_heap.span_mut())?;
            }
        }
        Ok(())
    }
}

unsafe impl<R, H> Send for ThreadHeap<R, H> {}

