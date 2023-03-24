use crate::one_way_mmap_heap::{Heap, OneWayMmapHeap};
use core::mem::MaybeUninit;
use core::ops::{Index, IndexMut};

pub struct DynArray<T, const N: usize> {
    pointers: *mut *mut T,
}

impl<T, const N: usize> DynArray<T, N> {
    pub fn create() -> Self {
        let size = core::mem::size_of::<Option<T>>() * N;
        let pointers = unsafe { OneWayMmapHeap.malloc(size) } as *mut Option<*mut T>;
        let pointer_slice =
            core::ptr::slice_from_raw_parts_mut(pointers, N) as *mut [Option<*mut T>; N];
        unsafe { pointer_slice.write([None; N]) };
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

    pub fn get(&self, index: usize) -> Option<Option<*mut T>> {
        if index <= N {
            let pointers = unsafe {
                core::ptr::slice_from_raw_parts_mut(self.pointers, N) as *mut [Option<*mut T>; N]
            };
            let pointers = unsafe { pointers.as_mut().unwrap() };
            Some(pointers[index])
        } else {
            None
        }
    }

    pub fn write_at(&mut self, at: usize, element: T) {
        if at < N {
            let size = core::mem::size_of::<T>();
            let ele = unsafe { OneWayMmapHeap.malloc(size) } as *mut T;
            unsafe { ele.write(element) };
            let slice = unsafe { self.as_mut_slice().as_mut().unwrap() };
            slice[at] = Some(ele);
        }
    }

    pub fn is_empty(&self) -> bool {
        let slice = unsafe { self.as_slice().as_ref().unwrap() };
        slice.iter().all(|x| x.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_retrieves() {
        unsafe {
            let mut slice = DynArray::<u32, 16>::create();
            let slice = slice.as_mut_slice().as_mut().unwrap();
            let heapyeine = OneWayMmapHeap.malloc(8) as *mut u32;
            heapyeine.write(1u32);
            slice[0] = Some(heapyeine);
            let heapyzwei = OneWayMmapHeap.malloc(8) as *mut u32;
            heapyzwei.write(2u32);
            slice[1] = Some(heapyzwei);
            assert_eq!(*slice[1].unwrap(), 2);
            assert!(slice[2].is_none());
        }
    }
}
