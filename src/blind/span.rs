use std::ptr::NonNull;

#[derive(Debug)]
pub struct Span {
    /// Span of pages this heap manages.
    data: NonNull<u8>,

    /// Length of the span's allocation in system pages.
    length: u32,

    /// Size of allocations stored (max 16K)
    size_class: u16,

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

    pub fn size_class(&self) -> u16 {
        self.size_class
    }

    pub fn span_length(&self) -> usize {
        self.max_allocations as usize * self.size_class as usize
    }

    pub fn assign_size_class(mut self, size_class: u16, alignment: u16) -> Self {
        let size_class = ((size_class + alignment + 1) / alignment) * alignment;

        self.size_class = size_class;

        let mut num_allocations = (self.length as usize * page_size::get()) / size_class as usize;
        self.max_allocations = if num_allocations > u8::MAX as usize {
            u8::MAX
        } else {
            num_allocations as u8
        };

        self
    }

    pub fn pointer_to(&mut self, offset: u8) -> Option<NonNull<u8>> {
        if offset >= self.max_allocations {
            return None;
        }



        // SAFETY:
        // - The starting pointer is valid (make new unsafe) and the offset is in the same allocated
        //   object.
        // - The offset is a u8 so will not overflow a isize (what about a self.data at the end of
        //   memory? condition on the new method)
        // - The offset will not wrap the memory space.
        unsafe {
            Some(NonNull::new(self.data.as_ptr().offset(offset as isize * self.size_class as isize)).unwrap())
        }
    }
}

pub trait SpanAllocator {
    fn allocate_span(&mut self, pages: u32) -> Span;
}

pub struct TestSpanAllocator;

impl SpanAllocator for TestSpanAllocator {
    fn allocate_span(&mut self, pages: u32) -> Span {
        let data = vec![0u8; pages as usize * page_size::get()];
        let data = data.into_boxed_slice();
        let data = Box::into_raw(data);
        let data = data as *mut u8;
        let data = NonNull::new(data).unwrap();

        eprintln!("Allocated pages: {}", pages);

        Span {
            data,
            max_allocations: 0,
            size_class: 0,
            length: pages,
        }
    }
}
