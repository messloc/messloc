use libc::{mmap, MAP_ANONYMOUS, MAP_FAILED, MAP_NORESERVE, MAP_PRIVATE, PROT_READ, PROT_WRITE};
use std::{process::abort, ptr::null_mut};

use crate::PAGE_SIZE;

pub trait Heap {
    // unsafe fn map(&mut self, size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut ();
    unsafe fn malloc(&mut self, size: usize) -> *mut ();
    unsafe fn get_size(&mut self, ptr: *mut ()) -> usize;
    unsafe fn free(&mut self, ptr: *mut ());
}

pub struct OneWayMmapHeap;

impl OneWayMmapHeap {
    pub unsafe fn map(&mut self, mut size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut () {
        if size == 0 {
            return null_mut();
        }

        // Round up to the size of a page.
        size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        let ptr = mmap(null_mut(), size, PROT_READ | PROT_WRITE, flags, fd, 0);
        if ptr == MAP_FAILED {
            // we probably shouldn't panic in allocators
            abort()
        }

        // debug_assert_eq!(ptr.align_offset(Self::ALIGNMENT), 0);

        ptr.cast()
    }
}

impl Heap for OneWayMmapHeap {
    unsafe fn malloc(&mut self, size: usize) -> *mut () {
        self.map(size, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1)
    }

    unsafe fn get_size(&mut self, _: *mut ()) -> usize {
        0
    }

    unsafe fn free(&mut self, _: *mut ()) {}
}
