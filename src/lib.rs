
#![cfg_attr(feature = "allocator-api", feature(allocator_api))]
#![warn(
    rust_2018_idioms,
    // missing_debug_implementations,
    // missing_docs,
)]

use std::{alloc::{GlobalAlloc, Layout}, ptr::NonNull};

#[cfg(feature = "allocator-api")]
use std::alloc::{Allocator, AllocError};

mod global_heap;
mod cheap_heap;
mod meshable_arena;

const PAGE_SIZE: usize = 4096;
const DATA_LEN: usize = 128;
const ARENA_SIZE: usize = PAGE_SIZE * 2;

const SPAN_CLASS_COUNT: u32 = 256;
const MIN_ARENA_EXPANSION: usize = 4096;  // 16 MB in pages

pub struct MiniHeap; // stub


pub struct Messloc {

}

impl Messloc {
    fn allocate(&self, layout: Layout) -> Option<NonNull<[u8]>> {
        todo!()
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        todo!()
    }
}

unsafe impl GlobalAlloc for Messloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match self.allocate(layout) {
            Some(p) => p.as_ptr() as *mut _,
            // Errors are indicated by null pointers
            None => std::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` is guaranteed to point to valid memory allocated
        // by this allocator.
        let ptr = NonNull::new_unchecked(ptr);
        self.deallocate(ptr, layout);
    }
}

#[cfg(feature = "allocator-api")]
unsafe impl Allocator for Messloc {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.allocate(layout).ok_or(AllocError)
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        self.deallocate(ptr, layout)
    }
}
