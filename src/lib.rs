#![cfg_attr(feature = "allocator-api", feature(allocator_api))]
#![warn(
    rust_2018_idioms,
    // missing_debug_implementations,
    // missing_docs,
)]
#![allow(clippy::needless_for_each)]
#![allow(clippy::module_name_repetitions)]
#![feature(allocator_api)]
#![feature(let_chains)]
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(if_let_guard)]
#![recursion_limit = "256"]
#![deny(clippy::pedantic)]

use core::{
    alloc::{GlobalAlloc, Layout},
};

use once_cell::sync::OnceCell;

pub use crate::runtime::Messloc;

#[cfg(feature = "allocator_api")]
use core::alloc::{AllocError, Allocator};

mod arena_fs;
mod class_array;
mod comparatomic;
mod fake_std;
mod global_heap;
mod meshable_arena;
mod mini_heap;
mod one_way_mmap_heap;
mod rng;
mod runtime;
mod shuffle_vector;
mod utils;

const PAGE_SIZE: usize = 4096;
const MAX_SMALL_SIZE: usize = 1024;
const MAX_SIZE: usize = 16384;
const NUM_BINS: usize = 25;
const MAX_SHUFFLE_VECTOR_LENGTH: usize = 64;
const MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR: usize = 24;

unsafe impl GlobalAlloc for Messloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.allocate(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` is guaranteed to point to valid memory allocated
        // by this allocator.

        self.deallocate(ptr, layout);
    }
}

pub struct MessyLock(pub once_cell::sync::OnceCell<Messloc>);

impl MessyLock {
    pub fn init_in_place(&self) {
        let _ = OnceCell::set(&self.0, Messloc::init());
        if OnceCell::get(&self.0).is_none() {}
    }
}

unsafe impl GlobalAlloc for MessyLock {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if once_cell::sync::OnceCell::get(&self.0).is_none() {
            self.init_in_place();
        }
        let ptr = OnceCell::get(&self.0).unwrap().allocate(layout);
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(lazy) = once_cell::sync::OnceCell::get(&self.0) {
            lazy.deallocate(ptr, layout);
        } else {
            unreachable!()
        }
    }
}

impl Drop for MessyLock {
    fn drop(&mut self) {}
}
