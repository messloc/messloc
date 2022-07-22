use core::marker::PhantomData;
use core::mem::size_of;
use core::ptr::copy_nonoverlapping;
use std::{mem::align_of, ptr::{drop_in_place, NonNull}};

use super::span::{Span, SpanAllocator, TestSpanAllocator};

#[derive(Debug)]
pub struct SpanVec<T, S>
where
    S: SpanAllocator,
{
    span_alloc: S,
    span: Span<S::Handle>,
    phantom: PhantomData<*const [T]>,
    length: usize,
    capacity: usize,
}

unsafe impl<T: Send, S: Send + SpanAllocator> Send for SpanVec<T, S> {}
unsafe impl<T: Sync, S: Sync + SpanAllocator> Sync for SpanVec<T, S> {}

pub fn div_ceil(a: usize, b: usize) -> usize {
    (a + b - 1) / b
}

impl<T, S> Drop for SpanVec<T, S>
where
    S: SpanAllocator,
{
    fn drop(&mut self) {
        // drop each item
        for offset in 0..self.length {
            unsafe {
                drop_in_place(
                    self.span
                        .data_ptr()
                        .as_ptr()
                        .cast::<T>()
                        .offset(offset as isize),
                )
            }
        }

        // deallocate the span
        let _ = unsafe { self.span_alloc.deallocate_span(&mut self.span) };
    }
}

impl<T, S> SpanVec<T, S>
where
    S: SpanAllocator,
{
    pub fn with_capacity_in(capacity: usize, mut span_alloc: S) -> Result<Self, S::AllocError> {
        let page_size = span_alloc.page_size();

        debug_assert!(page_size >= align_of::<T>());

        let pages = div_ceil(capacity * size_of::<T>(), page_size);
        let span = unsafe { span_alloc.allocate_span(pages.try_into().unwrap())? };

        let capacity = (pages * page_size) / size_of::<T>();

        Ok(Self {
            span_alloc,
            span,
            phantom: PhantomData,
            length: 0,
            capacity,
        })
    }

    fn reserve(&mut self, additional: usize) -> Result<(), S::AllocError> {
        let page_size = self.span_alloc.page_size();
        let pages = div_ceil((self.length + additional) * size_of::<T>(), page_size).max(1);

        if pages <= self.span.pages() as usize {
            return Ok(());
        }

        let span = unsafe { self.span_alloc.allocate_span(pages.try_into().unwrap())? };

        let capacity = (pages * page_size) / size_of::<T>();

        unsafe {
            copy_nonoverlapping(
                self.span.data_ptr().as_ptr().cast::<T>(),
                span.data_ptr().as_ptr().cast::<T>(),
                self.length,
            )
        };

        let mut old_span = core::mem::replace(&mut self.span, span);

        self.capacity = capacity;

        unsafe { self.span_alloc.deallocate_span(&mut old_span).map_err(|_| ()) };

        Ok(())
    }

    pub fn push(&mut self, value: T) -> Result<usize, (T, S::AllocError)> {
        if self.length == self.capacity {
            if let Err(err) = self.reserve(1) {
                return Err((value, err));
            }
        }

        let pointer: *mut T = unsafe {
            self.span
                .data_ptr()
                .as_ptr()
                .cast::<T>()
                .offset(self.length as isize)
        };

        unsafe { pointer.write(value) };

        let length = self.length;
        self.length += 1;

        Ok(length)
    }

    pub fn as_slice(&self) -> &[T] {
        unsafe { core::slice::from_raw_parts(self.span.data_ptr().as_ptr().cast(), self.length) }
    }

    pub fn as_slice_mut(&mut self) -> &mut [T] {
        unsafe {
            core::slice::from_raw_parts_mut(self.span.data_ptr().as_ptr().cast(), self.length)
        }
    }

    pub fn len(&self) -> usize {
        self.length
    }
}

#[test]
fn span_vec_alloc_dealloc() {
    let mut v = SpanVec::with_capacity_in(0, TestSpanAllocator).unwrap();

    assert_eq!(v.push("abc".to_string()), Ok(0));
    assert_eq!(v.push("def".to_string()), Ok(1));
    assert_eq!(v.push("ghi".to_string()), Ok(2));

    assert_eq!(
        v.as_slice(),
        &["abc".to_string(), "def".to_string(), "ghi".to_string()]
    );
    assert_eq!(
        v.as_slice_mut(),
        &mut ["abc".to_string(), "def".to_string(), "ghi".to_string()]
    );
    assert_eq!(v.len(), 3);

    v.reserve(200);
    assert_eq!(
        v.as_slice(),
        &["abc".to_string(), "def".to_string(), "ghi".to_string()]
    );
    assert_eq!(v.len(), 3);

    for i in 0..200 {
        assert_eq!(v.push("x".to_string()), Ok(i + 3));
    }

    assert_eq!(v.len(), 203);

    assert_eq!(v.capacity, (4096 * 2) / 24);
}
