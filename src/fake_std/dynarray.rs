use crate::one_way_mmap_heap::{Heap, OneWayMmapHeap};
use core::mem::MaybeUninit;
use core::ops::{Index, IndexMut};

pub struct DynArray<T, const N: usize> {
    pointers: *mut *mut T,
}

impl<T, const N: usize> DynArray<T, N> {
    pub fn create() -> Self {
        let size = core::mem::size_of::<*mut Option<T>>() * N;
        let pointers = unsafe { OneWayMmapHeap.malloc(size) } as *mut Option<*mut T>;
        let pointer_slice = unsafe {
            core::ptr::slice_from_raw_parts_mut(pointers, N)
                .as_mut()
                .unwrap()
        };
        (0..N).for_each(|n| {
            let element = unsafe { OneWayMmapHeap.malloc(core::mem::size_of::<Option<*mut T>>()) }
                as *mut Option<T>;
            unsafe { element.write(None) };
            pointer_slice[n] = Some(element.cast());
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

    pub unsafe fn get(&self, index: usize) -> Option<*mut Option<*mut T>> {
        if index <= N {
            let pointers =
                core::ptr::slice_from_raw_parts_mut(self.pointers, N) as *mut [Option<*mut T>; N];
            Some(pointers.add(index).cast::<Option<*mut T>>())
        } else {
            None
        }
    }

    pub fn is_empty(&self) -> bool {
        let slice = unsafe { self.as_slice().as_ref().unwrap() };
        slice.iter().all(|x| x.is_none())
    }
}
