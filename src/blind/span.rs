use std::{marker::PhantomData, ptr::NonNull};

/// A span of continuous pages.
///
/// If this value is dropped without explicitly deallocating, then the pages **will** be leaked.
#[derive(Debug)]
pub struct Span<H> {
    /// Span of pages this span manages.
    data: NonNull<u8>,

    /// Extra handle for allocator to use.
    handle: H,

    /// Length of the span's allocation in pages.
    pages: u16,

    /// State of the span.
    state: State,
}

impl<H> Drop for Span<H> {
    fn drop(&mut self) {
        if self.state != State::Invalid {
            eprintln!("Warning: Span dropped without deallocating it!");
        }
    }
}

// TODO: add this to the span allocator's safety requirements
unsafe impl<H> Send for Span<H> {}

#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum State {
    /// Span is normally allocated.
    Normal,

    /// Span has been merged with another span.
    Merged,

    /// Span does not point to valid data.
    Invalid,
}

impl<H> Span<H> {
    /// # Safety
    /// - `data` must be valid for `'a`.
    pub unsafe fn new(data: NonNull<u8>, handle: H, pages: u16) -> Self {
        Self {
            data,
            handle,
            pages,
            state: State::Normal,
        }
    }

    pub fn data_ptr(&self) -> NonNull<u8> {
        self.data
    }

    pub fn pages(&self) -> u16 {
        self.pages
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn set_state(&mut self, state: State) {
        self.state = state;
    }

    pub fn set_handle(&mut self, handle: H) {
        self.handle = handle;
    }

    pub fn handle(&self) -> &H {
        &self.handle
    }
}

/// Trait for type that can allocate spans of pages.
///
/// # Safety
/// - The value returned by `page_size` **cannot** change during the lifetime of an instance.
///     It is allowed to be different between instances.
/// - The spans created by the type **must** be aligned to a minimum of `page_size`.
pub unsafe trait SpanAllocator {
    type AllocError;
    type DeallocError;
    type MergeError;
    type Handle;

    /// Size of allocator's pages in bytes.
    fn page_size(&self) -> usize;

    /// Allocate a set of pages to form a span.
    ///
    /// # Safety
    /// - The returned span **cannot** be used after this instance is dropped.
    unsafe fn allocate_span(&self, pages: u16) -> Result<Span<Self::Handle>, Self::AllocError>;

    /// Deallocate a span.
    ///
    /// # Safety
    /// - The passed `span` **must** have been allocated by this instance.
    /// - **No** pointers/references into the span's pages will be dereferenced after this call.
    unsafe fn deallocate_span(&self, span: &Span<Self::Handle>) -> Result<(), Self::DeallocError>;

    /// Merge two spans together.
    ///
    /// If successful, `span_to_merge`'s status will be updated.
    ///
    /// # Safety
    /// - `span` and `span_to_merge` are of the **same** length.
    /// - `span` and `span_to_merge` **must** have no overlapping active values.
    unsafe fn merge_spans(
        &self,
        span: &Span<Self::Handle>,
        span_to_merge: &mut Span<Self::Handle>,
    ) -> Result<(), Self::MergeError>;
}

#[derive(Copy, Clone)]
pub struct TestSpanAllocator;

#[derive(Copy, Clone)]
#[repr(align(256))]
struct Align256([u8; 4096]);

unsafe impl SpanAllocator for TestSpanAllocator {
    type AllocError = ();
    type DeallocError = ();
    type MergeError = ();
    type Handle = ();

    fn page_size(&self) -> usize {
        4095
    }

    unsafe fn allocate_span(&self, pages: u16) -> Result<Span<Self::Handle>, ()> {
        let data = vec![Align256([0; 4096]); pages as usize];
        let data = data.into_boxed_slice();
        let data = Box::into_raw(data);
        let data = data as *mut u8;
        let data = NonNull::new(data).unwrap();

        eprintln!("Allocated pages: {}", pages);

        Ok(unsafe { Span::new(data, (), pages) })
    }

    unsafe fn deallocate_span(&self, span: &Span<Self::Handle>) -> Result<(), ()> {
        let slice = std::ptr::slice_from_raw_parts_mut(
            span.data.as_ptr().cast::<Align256>(),
            span.pages as usize,
        );
        Box::from_raw(slice);

        eprintln!("Deallocated pages: {}", span.pages());

        Ok(())
    }

    unsafe fn merge_spans(
        &self,
        span: &Span<Self::Handle>,
        span_to_merge: &mut Span<Self::Handle>,
    ) -> Result<(), ()> {
        Err(())
    }
}
