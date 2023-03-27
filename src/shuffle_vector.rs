use core::ptr::null_mut;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::{
    comparatomic::Comparatomic,
    fake_std::dynarray::DynArray,
    mini_heap::MiniHeap,
    rng::Rng,
};
use crate::MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR;

pub struct ShuffleVector<const N: usize> {
    pub start: *mut (),
    array: *mut (),
    pub mini_heaps: DynArray<MiniHeap, MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR>,
    pub mini_heap_count: usize,
    rng: Rng,
    pub offset: Comparatomic<AtomicU64>,
}
impl<const N: usize> ShuffleVector<N> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            start: null_mut(),
            array: null_mut(),
            mini_heaps: DynArray::create(),
            mini_heap_count: 0,
            rng: Rng::init(),
            offset: Comparatomic::new(0),
        }
    }

    //TODO: rework this to shuffle when required
    pub fn insert(&mut self, heap: *mut MiniHeap) {
        let mini_heaps = unsafe { self.mini_heaps.as_mut_slice().as_mut().unwrap() };
        let mini_heap = mini_heaps.iter().position(|x| x.is_none());
        match mini_heap {
            Some(n) => mini_heaps[n] = Some(heap),
            None => unreachable!(),
        }
    }

    pub fn local_refill(&mut self) -> bool {
        let mini_heaps = unsafe { self.mini_heaps.as_mut_slice().as_mut().unwrap() };

        mini_heaps
            .iter()
            .position(|x| x.is_none())
            .map(|x| self.rng.shuffle(self.array as *mut [Entry; N], x, N))
            .is_some()
    }

    pub fn malloc(&self) -> *mut MiniHeap {
        if !self.array.is_null() {
            let entry = self.pop();
            self.ptr_from_offset(&entry)
        } else {
            null_mut()
        }
    }

    pub fn pop(&self) -> Entry {
        let val = unsafe {
            self.array
                .cast::<Entry>()
                .add(self.offset.load(Ordering::Acquire) as usize)
                .read()
        };

        self.offset.fetch_add(1, Ordering::AcqRel);
        val
    }

    pub fn ptr_from_offset(&self, offset: &Entry) -> *mut MiniHeap {
        //TODO: reädd the assert check if offset is in the range

        unsafe {
            self.start
                .cast::<MiniHeap>()
                .add(offset.mh_offset)
                .add(offset.bit_offset)
        }
    }
}

unsafe impl<const N: usize> Send for ShuffleVector<N> {}

#[derive(Clone, Debug, Default)]
pub struct Entry {
    mh_offset: usize,
    bit_offset: usize,
}

impl Entry {
    #[must_use]
    pub const fn new(mh_offset: usize, bit_offset: usize) -> Self {
        Self {
            mh_offset,
            bit_offset,
        }
    }
}
