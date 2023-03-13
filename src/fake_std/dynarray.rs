use crate::fake_std::Initer;
use crate::one_way_mmap_heap::{Heap, OneWayMmapHeap};
use core::mem::MaybeUninit;
use core::ops::{Index, IndexMut};

pub struct DynArray<T: Initer, const N: usize> {
    pointers: *mut *mut T,
}

impl<T: Initer, const N: usize> DynArray<T, N> {
    pub fn create() -> Self {
        let size = core::mem::size_of::<*mut T>() * N;
        let pointers = unsafe { OneWayMmapHeap.malloc(size) } as *mut *mut T;
        let pointer_slice = unsafe {
            core::ptr::slice_from_raw_parts_mut(pointers, N)
                .as_mut()
                .unwrap()
        };
        (0..N).for_each(|n| {
            let element = unsafe { OneWayMmapHeap.malloc(core::mem::size_of::<Option<*mut T>>()) }
                as *mut Option<T>;
            unsafe { element.write(None) };
            pointer_slice[n] = element.cast();
        });

        Self {
            pointers: pointers.cast(),
        }
    }

    pub unsafe fn as_slice(&self) -> *const [Option<*mut T>] {
        core::ptr::slice_from_raw_parts(self.pointers.cast::<Option<*mut T>>(), N)
    }

    pub unsafe fn as_mut_slice(&mut self) -> *mut [Option<*mut T>] {
        core::ptr::slice_from_raw_parts_mut(self.pointers.cast::<Option<*mut T>>(), N)
    }

    pub fn inner(&self) -> *mut *mut T {
        self.pointers
    }

    pub unsafe fn get(&self, index: usize) -> *mut T {
        let pointers = core::ptr::slice_from_raw_parts_mut(self.pointers, N) as *mut [T; N];
        pointers.add(index).cast()
    }
}
