use crate::PAGE_SIZE;
use core::ptr::slice_from_raw_parts_mut;
use core::{alloc::Allocator, ptr::null_mut, ptr::NonNull};
use libc::{mmap, MAP_ANONYMOUS, MAP_FAILED, MAP_NORESERVE, MAP_PRIVATE, PROT_READ, PROT_WRITE};
use once_cell::race::OnceNonZeroUsize;

pub trait Heap {
    type PointerType;
    type MallocType;
    unsafe fn map(&mut self, size: usize, flags: libc::c_int, fd: libc::c_int)
        -> Self::PointerType;
    unsafe fn malloc(&mut self, size: usize) -> *mut Self::MallocType;
    unsafe fn grow<T>(&mut self, src: *mut T, old: usize, new: usize) -> *mut T;
    unsafe fn get_size(&mut self, ptr: *mut ()) -> usize;
    unsafe fn free(&mut self, ptr: *mut ());
}

pub struct OneWayMmapHeap;

impl Heap for OneWayMmapHeap {
    type PointerType = *mut ();
    type MallocType = ();
    unsafe fn map(&mut self, mut size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut () {
        if size == 0 {
            return null_mut();
        }

        // Round up to the size of a page.
        size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        //TODO:: use utils::mmap instead as we are currently using two
        //different forms of mmap
        let ptr = mmap(null_mut(), size, PROT_READ | PROT_WRITE, flags, fd, 0);

        if ptr == MAP_FAILED {
            // we probably shouldn't panic in allocators
            panic!()
        }

        // debug_assert_eq!(ptr.align_offset(Self::ALIGNMENT), 0);

        ptr.cast()
    }

    unsafe fn malloc(&mut self, size: usize) -> *mut () {
        self.map(size, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1)
    }

    unsafe fn grow<T>(&mut self, src: *mut T, old: usize, new: usize) -> *mut T {
        let alloc = self.malloc(new * core::mem::size_of::<T>()) as *mut T;
        alloc.write(src.read());
        alloc.cast()
    }

    unsafe fn get_size(&mut self, _: *mut ()) -> usize {
        0
    }

    unsafe fn free(&mut self, _: *mut ()) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    pub fn test_large_alloc() {
        unsafe {
            let big_array = [0u64; 2200];
            let heap = OneWayMmapHeap.malloc(core::mem::size_of_val(&big_array)) as *mut u64;
            let heap_slice = core::ptr::slice_from_raw_parts_mut(heap, 2200) as *mut [u64; 2200];
            let heap_slice = heap_slice.as_mut().unwrap();
            heap_slice.iter_mut().enumerate().for_each(|(i, sl)| {
                *sl = big_array[i] + i as u64;
            });

            heap_slice.iter_mut().for_each(|(sl)| {
                dbg!(sl);
            });
        }
    }
}
