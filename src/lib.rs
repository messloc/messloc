//! Better memory allocation using novel meshing algorithms
//!
//! Provides a better memory allocator than rustc's default allocator.
//! These are some of the advantages of messloc over rustc's default:
//! - Memory efficiency (less memory used while doing tasks)
//! - Drop-in replacement (no messing around required)
//!
//! 
//! 
//! Operating systems supported:
//!  - [x] Linux
//!  - [x] MacOS
//!  - BSD (BSDs have not been tested, please open an issue if messloc is working for you)
//!  - Windows (Work in progress)
//!
//! MAB: messloc requires Rust nightly. stable is not compatible with the features needed.




#![cfg_attr(feature = "allocator-api", feature(allocator_api))]
#![warn(
    rust_2018_idioms,
    // missing_debug_implementations,
    // missing_docs,
)]
#![allow(unused)]
#![feature(type_alias_impl_trait)]
#![feature(let_chains)]
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_array_assume_init)]
#![recursion_limit = "256"]
use std::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
};

use crate::runtime::Runtime;

#[cfg(feature = "allocator_api")]
use std::alloc::{AllocError, Allocator};

mod arena_fs;
mod atomic_enum;
mod bitmap;
mod cheap_heap;
mod class_array;
mod comparatomic;
mod global_heap;
mod internal;
mod list_entry;
mod meshable_arena;
mod mini_heap;
mod mmap_heap;
mod one_way_mmap_heap;
mod rng;
mod runtime;
mod shuffle_vector;
mod span;
mod splits;
mod thread_local_heap;
mod utils;

const PAGE_SIZE: usize = 4096;
const DATA_LEN: usize = 128;
#[cfg(target_os = "linux")]
const ARENA_SIZE: usize = 64 * 1024 * 1024 * 1024; // 64 GB
#[cfg(target_os = "macos")]
const ARENA_SIZE: usize = 32 * 1024 * 1024 * 1024; // 32 GB

const SPAN_CLASS_COUNT: u32 = 256;
const MIN_ARENA_EXPANSION: usize = 4096; // 16 MB in pages
const MAX_SMALL_SIZE: usize = 1024;
const MAX_SIZE: usize = 16384;
const MAP_SHARED: i32 = 1;
const DIRTY_PAGE_THRESHOLD: usize = 32;
const MAX_MESHES: usize = 256;
const MAX_MERGE_SETS: usize = 4096;
const MAX_SPLIT_LIST_SIZE: usize = 16384;
const NUM_BINS: usize = 25;
const DEFAULT_MAX_MESH_COUNT: usize = 30000;
const MAX_MESHES_PER_ITERATION: usize = 2500;
const OCCUPANCY_CUTOFF: f64 = 0.8;
const BINNED_TRACKER_MAX_EMPTY: u64 = 128;
const MESHES_PER_MAP: f64 = 0.33;
const MAX_SHUFFLE_VECTOR_LENGTH: usize = 256;
const MIN_OBJECT_SIZE: usize = 8;
const MINI_HEAP_REFILL_GOAL_SIZE: usize = 4 * 1024;
const MIN_STRING_LEN: usize = 8;
const ENABLED_SHUFFLE_ON_INIT: bool = true;
const MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR: usize = 24;

unsafe impl GlobalAlloc for Runtime {
    
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.allocate(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` is guaranteed to point to valid memory allocated
        // by this allocator.
        self.deallocate(ptr, layout);
    }
}

#[cfg(feature = "allocator-api")]
unsafe impl<'a> Allocator for Runtime<'a> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.allocate(layout).ok_or(AllocError)
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        self.deallocate(ptr, layout)
    }
}
