use std::marker::PhantomData;

use crate::consts::{PAGE_SIZE, SPAN_CLASS_COUNT};

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub offset: Offset,
    pub length: Length,
}
pub type Offset = u32;
pub type Length = u32;

impl Span {
    pub fn class(self) -> u32 {
        Length::min(self.length, SPAN_CLASS_COUNT) - 1
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

    pub fn is_empty(self) -> bool {
        self.length == 0
    }
    pub fn byte_length(self) -> usize {
        self.length as usize * PAGE_SIZE
    }
}

pub type Page = [u8; PAGE_SIZE];
