//! This is a blind implementation taken directly from the paper describing the allocator
//! https://raw.githubusercontent.com/plasma-umass/Mesh/master/mesh-pldi19-powers.pdf
//!
//! The author's orginal code was not used as reference for this implementation.

use std::ptr::NonNull;

#[test]
fn basic_test() {
    println!("System page size: {}", page_size::get());

    println!("MiniHeap size: {}", std::mem::size_of::<MiniHeap>());
    println!("MiniHeap alignment: {}", std::mem::align_of::<MiniHeap>());
    println!("MiniHeap unused bytes: {}", std::mem::size_of::<MiniHeap>() - 327);

    panic!();
}

struct GlobalHeap {
    unattached_mini_heap_bins: Vec<Vec<MiniHeap>>,
}

/// Max number of virtual spans that can be overlayed onto a span
const MAX_OVERLAP: usize = 3;

/// Max allocations each span can contain
const MAX_ALLOCATIONS_PER_SPAN: usize = 255;

struct MiniHeap {
    /// Virtual spans overlayed onto the main span
    virtual_spans: [Option<NonNull<u8>>; MAX_OVERLAP],

    /// Span of pages this heap manages
    span: Span,

    /// Vector of randomized free allocation offsets
    shuffle_vector: ShuffleVector<MAX_ALLOCATIONS_PER_SPAN>,

    /// Mask of the current allocations
    allocation_mask: AllocationMask<{(MAX_ALLOCATIONS_PER_SPAN + 7) / 8}>,
}

struct ShuffleVector<const COUNT: usize> {
    data: [u8; COUNT],
    offset: u8,
}

impl<const COUNT: usize> ShuffleVector<COUNT> {
    fn new<R: rand::Rng>(rng: &mut R) -> Self {
        Self {
            data: [(); COUNT].map(|_| 0),
            offset: 0,
        }
    }
}

struct AllocationMask<const COUNT: usize> {
    mask: [core::sync::atomic::AtomicU8; COUNT]
}

impl<const COUNT: usize> AllocationMask<COUNT> {
    fn new() -> Self {
        Self {
            mask: [(); COUNT].map(|_| core::sync::atomic::AtomicU8::new(0)),
        }
    }
}

struct Span {
    /// Span of pages this heap manages.
    data: NonNull<u8>,

    /// Length of the span's allocation in system pages.
    length: u32,

    /// Size of allocations stored (max 16K)
    allocation_size: u16,

    /// Length of the span in number of allocations
    max_allocations: u8,
}

impl Span {
    pub fn memory_usage(&self) -> usize {
        self.length as usize * page_size::get()
    }

    pub fn max_allowed_allocations(&self) -> u8 {
        self.max_allocations
    }

    pub fn allocation_size(&self) -> u16 {
        self.allocation_size
    }

    pub fn span_length(&self) -> usize {
        self.max_allocations as usize * self.allocation_size as usize
    }
}

trait SpanAllocator {
    fn allocate_span(&mut self, allocation_size: u16, max_allocations: u8) -> Span;
}

struct TestSpanAllocator;

impl SpanAllocator for TestSpanAllocator {
    fn allocate_span(&mut self, allocation_size: u16, max_allocations: u8) -> Span {
        let data = vec![0u8; allocation_size as usize * max_allocations as usize];
        let length = (data.len() / page_size::get()) as u32;
        assert_eq!(length as usize * page_size::get(), data.len());
        let data = data.into_boxed_slice();
        let data = Box::into_raw(data);
        let data = unsafe { &mut (*data)[0] as *mut _ };
        let data = NonNull::new(data).unwrap();

        Span {
            data,
            allocation_size,
            max_allocations,
            length,
        }
    }
}

impl MiniHeap {
    pub fn new<R: rand::Rng>(span: Span, rng: &mut R) -> Self {
        Self {
            span,
            allocation_mask: AllocationMask::new(),
            shuffle_vector: ShuffleVector::new(rng),
            virtual_spans: [None; MAX_OVERLAP],
        }
    }
}
