use std::{alloc::Layout, ptr::{NonNull, slice_from_raw_parts_mut}};

use rand::Rng;

use super::{
    allocation_mask::{AllocationMask, AllocationMaskFreeIter},
    shuffle_vec::ShuffleVector,
    span::Span,
};

/// Max number of virtual spans that can be overlayed onto a span
const MAX_OVERLAP: usize = 3;

/// Max allocations each span can contain
pub const MAX_ALLOCATIONS_PER_SPAN: usize = 254;

const MASK_SIZE: usize = (MAX_ALLOCATIONS_PER_SPAN + 7) / 8;

#[derive(Debug)]
pub struct MiniHeap<H> {
    /// Virtual spans overlayed onto the main span
    virtual_spans: [Option<NonNull<u8>>; MAX_OVERLAP],

    /// Span of pages this heap manages
    span: Span<H>,

    /// Size of allocations for this heap
    size_class: u16,

    /// Max number of allocations the span can hold for the size class
    max_allocations: u8,

    /// Mask of the current allocations
    allocation_mask: AllocationMask<MASK_SIZE>,
}

impl<H> MiniHeap<H> {
    pub unsafe fn new(span: Span<H>, page_size: usize, size_class: u16) -> Self {
        let max_allocations = ((span.pages() as usize * page_size) / size_class as usize).min(u8::MAX as usize).try_into().unwrap();

        Self {
            span,
            allocation_mask: AllocationMask::new(),
            virtual_spans: [None; MAX_OVERLAP],
            size_class,
            max_allocations,
        }
    }

    /// # Safety
    /// - `offset` must be free as given by the `free_iter`.
    pub unsafe fn alloc(&mut self, offset: u8) -> NonNull<[u8]> {
        self.allocation_mask.used(offset);
        let pointer = unsafe {
            self.span
                .data_ptr()
                .as_ptr()
                .offset(offset as isize * self.size_class as isize)
        };
        let slice_ptr = slice_from_raw_parts_mut(pointer, self.size_class as usize);
        unsafe { NonNull::new_unchecked(slice_ptr) }
    }

    pub unsafe fn dealloc(&mut self, offset: u8) {
        self.allocation_mask.free(offset);
    }

    pub fn free_iter(&self) -> AllocationMaskFreeIter<'_, MASK_SIZE> {
        self.allocation_mask.free_iter(self.max_allocations)
    }

    pub fn span(&self) -> &Span<H> {
        &self.span
    }

    pub unsafe fn span_mut(&mut self) -> &mut Span<H> {
        &mut self.span
    }
}

#[test]
fn basic_test() {
    use std::mem::*;

    assert_eq!(size_of::<MiniHeap<u64>>(), 88);
    assert_eq!(size_of::<Option<MiniHeap<u64>>>(), 88);
    assert_eq!(align_of::<MiniHeap<u64>>(), 8);
}
