use std::{alloc::Layout, ptr::NonNull};

use super::{allocation_mask::AllocationMask, shuffle_vec::ShuffleVector, span::Span};

/// Max number of virtual spans that can be overlayed onto a span
const MAX_OVERLAP: usize = 3;

/// Max allocations each span can contain
const MAX_ALLOCATIONS_PER_SPAN: usize = 255;

#[derive(Debug)]
pub struct MiniHeap {
    /// Virtual spans overlayed onto the main span
    virtual_spans: [Option<NonNull<u8>>; MAX_OVERLAP],

    /// Span of pages this heap manages
    span: Span,

    /// Vector of randomized free allocation offsets
    shuffle_vector: ShuffleVector<MAX_ALLOCATIONS_PER_SPAN>,

    /// Mask of the current allocations
    allocation_mask: AllocationMask<{ (MAX_ALLOCATIONS_PER_SPAN + 7) / 8 }>,
}

impl MiniHeap {
    pub fn new<R: rand::Rng>(span: Span, rng: &mut R) -> Self {
        let shuffle_vector = ShuffleVector::new(rng, span.max_allowed_allocations());
        Self {
            span,
            allocation_mask: AllocationMask::new(),
            shuffle_vector,
            virtual_spans: [None; MAX_OVERLAP],
        }
    }

    pub fn alloc(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        if let Some(offset) = self.shuffle_vector.pop() {
            self.allocation_mask.set(offset);
            Some(self.span.pointer_to(offset).unwrap())
        } else {
            // No more offsets to allocate in
            None
        }
    }

    pub fn span(&self) -> &Span {
        &self.span
    }
}

#[test]
fn basic_test() {
    println!("System page size: {}", page_size::get());

    println!("MiniHeap size: {}", std::mem::size_of::<MiniHeap>());
    println!("MiniHeap alignment: {}", std::mem::align_of::<MiniHeap>());
    println!(
        "MiniHeap unused bytes: {}",
        std::mem::size_of::<MiniHeap>() - 327
    );

    panic!();
}
