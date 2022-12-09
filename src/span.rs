use core::mem::MaybeUninit;

use arrayvec::ArrayVec;

use crate::{
    one_way_mmap_heap::{Heap, OneWayMmapHeap},
    PAGE_SIZE, SPAN_CLASS_COUNT,
};

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Span {
    pub offset: Offset,
    pub length: Length,
}
pub type Offset = usize;
pub type Length = usize;

impl Span {
    pub const fn new(offset: usize, length: usize) -> Self {
        Self { offset, length }
    }

    pub fn class(&self) -> usize {
        Length::min(self.length, SPAN_CLASS_COUNT) - 1
    }

    pub const fn byte_length(&self) -> usize {
        self.length * PAGE_SIZE
    }

    pub fn split_after(&mut self, page_count: Length) -> Self {
        debug_assert!(page_count <= self.length);
        let rest_page_count = self.length - page_count;
        self.length = page_count;
        Self {
            offset: self.offset + page_count,
            length: rest_page_count,
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }
}

#[allow(clippy::module_name_repetitions)]
pub struct SpanList<const INNER_COUNT: usize, const SPAN_COUNT: usize>(
    [ArrayVec<Span, INNER_COUNT>; SPAN_COUNT],
);

impl<const INNER_COUNT: usize, const SPAN_COUNT: usize> SpanList<INNER_COUNT, SPAN_COUNT> {
    pub fn alloc_new() -> *mut Self {
        let size = core::mem::size_of::<Self>();
        let alloc = unsafe { OneWayMmapHeap.malloc(size) as *mut Span };

        (0..SPAN_COUNT).for_each(|span| unsafe {
            let element = alloc.add(span) as *mut Span;
            element.write(Span::default());
        });
        alloc.cast()
    }

    pub fn inner(&self) -> &[ArrayVec<Span, INNER_COUNT>; SPAN_COUNT] {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut [ArrayVec<Span, INNER_COUNT>; SPAN_COUNT] {
        &mut self.0
    }

    pub fn get(&self, index: usize) -> Option<&ArrayVec<Span, INNER_COUNT>> {
        self.inner().get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut ArrayVec<Span, INNER_COUNT>> {
        self.0.get_mut(index)
    }

    pub fn len(&self) -> usize {
        self.inner().len()
    }

    pub fn clear(&mut self) {
        let _ = core::mem::take(self);
    }

    pub fn for_each_free<F>(&self, mut func: F)
    where
        F: FnMut(&Span),
    {
        #[allow(clippy::redundant_closure)]
        self.0.iter().flatten().for_each(|span| func(span));
    }
}

impl<const IC: usize, const SC: usize> Default for SpanList<IC, SC> {
    fn default() -> Self {
        let list = core::array::from_fn(|_| ArrayVec::default());

        Self(list)
    }
}
