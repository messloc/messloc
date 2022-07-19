use std::{alloc::Layout, ptr::NonNull};

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
    pub fn new(span: Span<H>, size_class: u16) -> Self {
        let max_allocations = (span.pages() / size_class).try_into().unwrap();

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
    pub unsafe fn alloc(&mut self, offset: u8) -> NonNull<u8> {
        self.allocation_mask.used(offset);
        unsafe {
            NonNull::new_unchecked(
                self.span
                    .data_ptr()
                    .as_ptr()
                    .offset(offset as isize * self.size_class as isize),
            )
        }
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
}

#[test]
fn basic_test() {
    use std::mem::*;

    assert_eq!(size_of::<MiniHeap<u64>>(), 88);
    assert_eq!(size_of::<Option<MiniHeap<u64>>>(), 88);
    assert_eq!(align_of::<MiniHeap<u64>>(), 8);
}
