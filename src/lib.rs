#![cfg_attr(feature = "allocator-api", feature(allocator_api))]
#![warn(
    rust_2018_idioms,
    // missing_debug_implementations,
    // missing_docs,
)]

use std::{
    alloc::{GlobalAlloc, Layout},
    ptr::{null_mut, NonNull},
};

#[cfg(feature = "allocator-api")]
use std::alloc::{AllocError, Allocator};

use global_heap::GlobalHeap;
use lazy_static::lazy::Lazy;

mod atomic_bitmap;
mod cheap_heap;
mod class_array;
mod global_heap;
mod internal;
mod meshable_arena;
mod mini_heap;
mod mmap_heap;
mod one_way_mmap_heap;
mod thread_local_heap;

const PAGE_SIZE: usize = 4096;
const DATA_LEN: usize = 128;
#[cfg(target_os = "linux")]
const ARENA_SIZE: usize = 64 * 1024 * 1024 * 1024; // 64 GB
#[cfg(target_os = "macos")]
const ARENA_SIZE: usize = 32 * 1024 * 1024 * 1024; // 32 GB

const SPAN_CLASS_COUNT: u32 = 256;
const MIN_ARENA_EXPANSION: usize = 4096; // 16 MB in pages
const MAX_SMALL_SIZE: usize = 1024;

pub struct Messloc;

impl Messloc {
    fn global(&self) -> &'static GlobalHeap {
        static LAZY: Lazy<GlobalHeap> = Lazy::INIT;
        LAZY.get(|| GlobalHeap::new())
    }

    fn allocate(&self, layout: Layout) -> Option<NonNull<[u8]>> {
        todo!()
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        todo!()
    }
    fn allocate_zeroed(&self, layout: Layout) -> Option<NonNull<[u8]>> {
        todo!()
    }
    unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Option<NonNull<[u8]>> {
        todo!()
    }
    unsafe fn grow_zeroed(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Option<NonNull<[u8]>> {
        todo!()
    }
    unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Option<NonNull<[u8]>> {
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

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        match self.allocate_zeroed(layout) {
            Some(ptr) => ptr.as_ptr().cast(),
            None => null_mut(),
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, old_layout: Layout, new_size: usize) -> *mut u8 {
        let ptr = NonNull::new_unchecked(ptr);
        let new_layout = Layout::from_size_align_unchecked(new_size, old_layout.align());

        let ptr = if old_layout.size() > new_layout.size() {
            self.shrink(ptr, old_layout, new_layout).map(NonNull::cast)
        } else if old_layout.size() < new_layout.size() {
            self.grow(ptr, old_layout, new_layout).map(NonNull::cast)
        } else {
            Some(ptr)
        };

        match ptr {
            Some(ptr) => ptr.as_ptr(),
            None => null_mut(),
        }
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
    fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.allocate_zeroed(layout).ok_or(AllocError)
    }
    unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        self.allocate_zeroed(layout).ok_or(AllocError)
    }
    unsafe fn grow_zeroed(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        self.grow_zeroed(ptr, old_layout, new_layout)
            .ok_or(AllocError)
    }
    unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<NonNull<[u8]>, AllocError> {
        self.shrink(ptr, old_layout, new_layout).ok_or(AllocError)
    }
}
