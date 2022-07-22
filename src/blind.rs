//! This is a blind implementation taken directly from the paper describing the allocator
//! https://raw.githubusercontent.com/plasma-umass/Mesh/master/mesh-pldi19-powers.pdf
//!
//! The author's orginal code was not used as reference for this implementation.

pub mod allocation_mask;
// pub mod linked_heap;
pub mod global_heap;
pub mod mini_heap;
pub mod shuffle_vec;
pub mod size_class;
pub mod span;
pub mod span_vec;
pub mod system_span_alloc;
pub mod thread_heap;
// pub mod local_heap;
use std::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ptr::{null_mut, slice_from_raw_parts_mut, NonNull},
    thread::ThreadId,
};

use allocation_mask::*;
use rand::SeedableRng;
// use linked_heap::*;
pub use global_heap::*;
use mini_heap::*;
use rand::Rng;
use shuffle_vec::*;
use span::*;
pub use span_vec::*;
use spin::{Lazy, Mutex, Once, RwLock, RwLockReadGuard, RwLockUpgradableGuard};
pub use system_span_alloc::*;
pub use thread_heap::*;
use thread_local::ThreadLocal;

const SIZE_CLASS_COUNT: usize = 25;

static SIZE_CLASSES: [u16; SIZE_CLASS_COUNT] = [
    8, 16, 32, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896,
    1024, 2048, 4096, 8192, 16384,
];

use rand_xoshiro::Xoshiro256Plus;

pub struct Messloc {
    heap: Once<GlobalHeap<Xoshiro256Plus, SystemSpanAlloc>>,
}

impl Messloc {
    pub const fn new() -> Self {
        Self { heap: Once::new() }
    }

    fn create_global_heap() -> GlobalHeap<Xoshiro256Plus, SystemSpanAlloc> {
        let span_alloc = SystemSpanAlloc::get();
        GlobalHeap::new(span_alloc, Xoshiro256Plus::seed_from_u64(1234568123987)).unwrap()
    }
}

unsafe impl GlobalAlloc for Messloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Ok(span) = self.heap.call_once(Self::create_global_heap).alloc(layout) {
            span.as_ptr().cast()
        } else {
            null_mut()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // eprintln!("dealloc!");
        // let span = unsafe { Span::new(NonNull::new(ptr).unwrap(), -1, 1) };
        //
        // self.heap.span_alloc.deallocate_span(&span);
    }
}
