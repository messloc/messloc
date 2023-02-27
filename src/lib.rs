#![cfg_attr(feature = "allocator-api", feature(allocator_api))]
#![warn(
    rust_2018_idioms,
    // missing_debug_implementations,
    // missing_docs,
)]
#![allow(unused)]
#![allow(clippy::needless_for_each)]
#![allow(clippy::module_name_repetitions)]
#![feature(type_alias_impl_trait)]
#![feature(let_chains)]
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(assert_matches)]
#![feature(allocator_api)]
#![feature(slice_ptr_len)]
#![feature(ptr_metadata)]
#![feature(set_ptr_value)]
#![feature(ptr_as_uninit)]
#![feature(if_let_guard)]
#![recursion_limit = "256"]

use core::{
    alloc::{AllocError, Allocator, GlobalAlloc, Layout},
    ptr::NonNull,
};

use once_cell::sync::OnceCell;

pub use crate::runtime::Messloc;

#[cfg(feature = "allocator_api")]
use core::alloc::{AllocError, Allocator};

mod arena_fs;
mod bitmap;
mod cheap_heap;
mod class_array;
mod comparatomic;
mod flags;
mod fake_std;
pub mod global_heap;
mod list_entry;
pub mod meshable_arena;
mod mini_heap;
mod mmap_heap;
mod one_way_mmap_heap;
mod rng;
mod runtime;
pub mod shuffle_vector;
mod span;
mod splits;
mod utils;

const DATA_LEN: usize = 128;
const PAGE_SIZE: usize = 4096;
#[cfg(target_os = "linux")]
const ARENA_SIZE: usize = 64 * 512 * 1024 * 1024;
// const ARENA_SIZE: usize = 64 * 1024 * 1024 * 1024; // 64 GB
#[cfg(target_os = "macos")]
const ARENA_SIZE: usize = 32 * 1024 * 1024 * 1024; // 32 GB

const SPAN_CLASS_COUNT: usize = 256;
const MIN_ARENA_EXPANSION: usize = 4096; // 16 MB in pages
const MAX_SMALL_SIZE: usize = 1024;
const MAX_SIZE: usize = 16384;
const MAP_SHARED: i32 = 1;
const DIRTY_PAGE_THRESHOLD: usize = 32;
const MAX_MESHES: usize = 256;
const MAX_MERGE_SETS: usize = 4096;
const MAX_SPLIT_LIST_SIZE: usize = 1024;
const NUM_BINS: usize = 25;
const DEFAULT_MAX_MESH_COUNT: usize = 30000;
const MAX_MESHES_PER_ITERATION: usize = 2500;
const OCCUPANCY_CUTOFF: f64 = 0.8;
const BINNED_TRACKER_MAX_EMPTY: u64 = 128;
const MESHES_PER_MAP: f64 = 0.33;
const MAX_SHUFFLE_VECTOR_LENGTH: usize = 64;
const MIN_OBJECT_SIZE: usize = 8;
const MINI_HEAP_REFILL_GOAL_SIZE: usize = 4 * 1024;
const MIN_STRING_LEN: usize = 8;
const ENABLED_SHUFFLE_ON_INIT: bool = true;
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
        OnceCell::set(&self.0, Messloc::init());
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
