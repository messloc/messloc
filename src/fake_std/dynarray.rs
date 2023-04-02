use crate::one_way_mmap_heap::OneWayMmapHeap;

pub struct DynArray<T, const N: usize> {
    pointers: *mut Option<*mut T>,
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

    pub fn as_slice(&self) -> *const [Option<*mut T>] {
        core::ptr::slice_from_raw_parts(self.pointers.cast::<Option<*mut T>>(), N)
    }

    pub fn as_mut_slice(&mut self) -> *mut [Option<*mut T>] {
        core::ptr::slice_from_raw_parts_mut(self.pointers.cast::<Option<*mut T>>(), N)
    }

    pub fn inner(&self) -> *mut Option<*mut T> {
        self.pointers
    }

    #[allow(clippy::option_option)]
    pub fn get(&self, index: usize) -> Option<Option<*mut T>> {
        if index <= N {
            let pointers =
                core::ptr::slice_from_raw_parts_mut(self.pointers, N) as *mut [Option<*mut T>; N];
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

    #[allow(clippy::redundant_closure_for_method_calls)]
    pub fn is_empty(&self) -> bool {
        let slice = unsafe { self.as_slice().as_ref().unwrap() };
        slice.iter().all(|x| x.is_none())
    }
}

pub struct DynDeq<T, const N: usize> {
    pointers: *mut Option<*mut T>,
    current: usize,
}

impl<T, const N: usize> DynDeq<T, N> {
    pub fn create() -> Self {
        let size = core::mem::size_of::<Option<T>>() * N;
        let pointers = unsafe { OneWayMmapHeap.malloc(size) } as *mut Option<*mut T>;
        let pointer_slice =
            core::ptr::slice_from_raw_parts_mut(pointers, N) as *mut [Option<*mut T>; N];
        unsafe { pointer_slice.write([None; N]) };
        Self {
            pointers: pointers.cast(),
            current: 0,
        }
    }

    pub fn as_slice(&self) -> *const [Option<*mut T>] {
        core::ptr::slice_from_raw_parts(self.pointers.cast(), N)
    }

    pub fn as_mut_slice(&self) -> *mut [Option<*mut T>] {
        core::ptr::slice_from_raw_parts_mut(self.pointers.cast(), N)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Option<*mut T>> {
        let slice = unsafe { self.as_slice().as_ref().unwrap() };
        slice[self.current..]
            .iter()
            .chain(slice[0..self.current].iter())
    }

    #[allow(clippy::redundant_closure_for_method_calls)]
    pub fn is_empty(&self) -> bool {
        self.iter().all(|x| x.is_none())
    }

    #[allow(clippy::redundant_closure_for_method_calls)]
    pub fn is_full(&self) -> bool {
        self.iter().all(|x| x.is_some())
    }

    pub fn capacity(&self) -> usize {
        self.iter().filter(|x| x.is_some()).count()
    }

    #[allow(clippy::option_option)]
    pub fn pop(&mut self) -> Option<Option<*mut T>> {
        if self.is_empty() {
            None
        } else {
            let ele = unsafe { *(self.pointers.add(self.current)) };
            self.current = (self.current + 1) % N;
            Some(ele)
        }
    }

    //TODO: Consider the weird case of pushing the same memory location to different slots, and
    //whether we need to handle that or not
    pub fn push(&self, val: *mut T) -> Option<()> {
        if self.is_full() {
            None
        } else {
            let slice = unsafe { self.as_mut_slice().as_mut().unwrap() };

            let slot = slice.iter_mut().find(|x| x.is_none())?;
            *slot = Some(val);

            Some(())
        }
    }

    pub fn swap_indices(&self, first: usize, second: usize) {
        if first < N && second < N {
            let slice = unsafe { self.as_mut_slice().as_mut().unwrap() };
            slice.swap(first, second);
        } else {
            panic!("at least one of the indices are bigger than the collection length");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::assert_matches::assert_matches;

    #[test]
    fn create_a_dynarray_and_slice() {
        let dynarray = DynArray::<u32, 4>::create();
        let slice = unsafe {
            core::ptr::slice_from_raw_parts(dynarray.pointers, 4)
                .as_ref()
                .unwrap()
        };
        assert_eq!(slice, &[None, None, None, None]);
        let slice = unsafe { dynarray.as_slice().cast::<*const [Option<u32>; 4]>().read() };
        unsafe { assert_eq!(*slice, [None, None, None, None]) };
    }

    #[test]
    fn create_a_mutable_slice() {
        let mut dynarray = DynArray::<u32, 4>::create();
        let slice = unsafe {
            dynarray
                .as_mut_slice()
                .cast::<*const [Option<u32>; 4]>()
                .read()
        };
        unsafe { assert_eq!(*slice, [None, None, None, None]) };
    }

    #[test]
    fn how_does_a_zst_fare() {
        let dynarray = DynArray::<u32, 0>::create();
        assert!(dynarray.pointers.is_null());
        let dyndeq = DynDeq::<u32, 0>::create();
        assert!(dyndeq.pointers.is_null());
        assert_eq!(dyndeq.current, 0);
    }

    #[test]
    fn get_retrieves() {
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

    #[test]
    fn is_empty() {
        let dynarray = DynArray::<u32, 4>::create();
        assert!(dynarray.is_empty());
    }

    #[test]
    fn writes_work() {
        let mut dynarray = DynArray::<u32, 4>::create();
        let slice = unsafe { dynarray.as_mut_slice().as_mut().unwrap() };
        let d = unsafe { OneWayMmapHeap.malloc(core::mem::size_of::<u32>()) } as *mut u32;
        unsafe { d.write(1u32) };
        slice[0] = Some(d);
        assert_matches!(slice[0], Some(c) if c == d);
    }

    #[test]
    fn create_a_dyndeq_and_slice() {
        let dynarray = DynDeq::<u32, 4>::create();
        let slice = unsafe {
            core::ptr::slice_from_raw_parts(dynarray.pointers, 4)
                .as_ref()
                .unwrap()
        };
        assert_eq!(slice, &[None, None, None, None]);
        let slice = unsafe { dynarray.as_slice().cast::<*const [Option<u32>; 4]>().read() };
        unsafe { assert_eq!(*slice, [None, None, None, None]) };
        dbg!(dynarray.current, 0);
    }

    #[test]
    fn create_a_mutable_slice_for_dyndeq() {
        let dynarray = DynDeq::<u32, 4>::create();
        let slice = unsafe {
            dynarray
                .as_mut_slice()
                .cast::<*const [Option<u32>; 4]>()
                .read()
        };
        unsafe { assert_eq!(*slice, [None, None, None, None]) };
        dbg!(dynarray.current, 0);
    }

    #[test]
    fn dyndeq_retrieves() {
        unsafe {
            let slice = DynDeq::<u32, 16>::create();
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

    #[test]
    fn dyndeq_is_empty() {
        let dynarray = DynDeq::<u32, 4>::create();
        assert!(dynarray.is_empty());
        assert_eq!(dynarray.current, 0);
    }

    #[test]
    fn dyndeq_writes_work() {
        let dynarray = DynDeq::<u32, 4>::create();
        let slice = unsafe { dynarray.as_mut_slice().as_mut().unwrap() };
        let d = unsafe { OneWayMmapHeap.malloc(core::mem::size_of::<u32>()) } as *mut u32;
        unsafe { d.write(1u32) };
        slice[0] = Some(d);
        assert_matches!(slice[0], Some(c) if c == d);
        assert_eq!(dynarray.current, 0);
    }

    #[test]
    fn dyndeq_pop_pops_and_increments_counter() {
        let mut dyndeq = DynDeq::<u32, 4>::create();
        let slice = unsafe { dyndeq.as_mut_slice().as_mut().unwrap() };
        let d1 = unsafe { OneWayMmapHeap.malloc(core::mem::size_of::<u32>()) } as *mut u32;
        unsafe { d1.write(1u32) };
        let d2 = unsafe { OneWayMmapHeap.malloc(core::mem::size_of::<u32>()) } as *mut u32;
        unsafe { d2.write(2u32) };
        slice[0] = Some(d1);
        slice[1] = Some(d2);
        let poppy = dyndeq.pop().unwrap();
        assert_matches!(poppy, Some(c) if c == d1);
        assert_eq!(dyndeq.current, 1);
    }
}
