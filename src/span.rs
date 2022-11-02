use std::mem::MaybeUninit;

use arrayvec::ArrayVec;

use crate::{PAGE_SIZE, SPAN_CLASS_COUNT};

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Span {
    pub offset: Offset,
    pub length: Length,
}
pub type Offset = u32;
pub type Length = u32;

impl Span {
    pub fn new(offset: u32, length: u32) -> Span {
        Span { offset, length }
    }

    pub fn class(&self) -> u32 {
        Length::min(self.length, SPAN_CLASS_COUNT) - 1
    }

    pub fn byte_length(&self) -> usize {
        usize::try_from(self.length).unwrap() * PAGE_SIZE
    }

    pub fn split_after(&mut self, page_count: Length) -> Self {
        debug_assert!(page_count <= self.length);
        let rest_page_count = self.length - page_count;
        self.length = page_count;
        Span {
            offset: self.offset + page_count,
            length: rest_page_count,
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }
}

#[allow(clippy::module_name_repetitions)]
pub struct SpanList([ArrayVec<Span, 1024>; SPAN_CLASS_COUNT as usize]);

impl SpanList {
    pub fn inner(&self) -> &[ArrayVec<Span, 1024>; SPAN_CLASS_COUNT as usize] {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut [ArrayVec<Span, 1024>; SPAN_CLASS_COUNT as usize] {
        &mut self.0
    }

    pub fn get(&self, index: usize) -> Option<&ArrayVec<Span, 1024>> {
        self.inner().get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut ArrayVec<Span, 1024>> {
        self.0.get_mut(index)
    }

    pub fn len(&self) -> usize {
        self.inner().len()
    }

    pub fn clear(&mut self) {
        let _ = std::mem::take(self);
    }

    pub fn for_each_free<F>(&self, mut func: F)
    where
        F: FnMut(&Span),
    {
        #[allow(clippy::redundant_closure)]
        self.0.iter().flatten().for_each(|span| func(span));
    }
}

impl Default for SpanList {
    fn default() -> SpanList {
        let list = {
            let mut list: [MaybeUninit<ArrayVec<Span, 1024>>; SPAN_CLASS_COUNT as usize] =
                unsafe { MaybeUninit::uninit().assume_init() };
            (0..SPAN_CLASS_COUNT as usize).for_each(|item| {
                let inner_vec = ArrayVec::default();

                list[item].write(inner_vec);
            });

            unsafe {
                std::mem::transmute::<_, [ArrayVec<Span, 1024>; SPAN_CLASS_COUNT as usize]>(list)
            }
        };

        SpanList(list)
    }
}
