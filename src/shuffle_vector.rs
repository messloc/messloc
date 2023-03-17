use core::ops::DerefMut;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use crate::list_entry::ListEntry;
use crate::one_way_mmap_heap::{Heap, OneWayMmapHeap};
use crate::{
    bitmap::{AtomicBitmapBase, Bitmap},
    comparatomic::Comparatomic,
    fake_std::dynarray::DynArray,
    mini_heap::MiniHeap,
    rng::Rng,
};
use crate::{ENABLED_SHUFFLE_ON_INIT, MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR};

pub struct ShuffleVector<const N: usize> {
    pub start: *mut (),
    array: *mut (),
    pub mini_heaps: DynArray<MiniHeap, MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR>,
    pub mini_heap_count: usize,
    rng: Rng,
    pub offset: Comparatomic<AtomicU64>,
    attached_offset: Comparatomic<AtomicU64>,
    object_size: usize,
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
            attached_offset: Comparatomic::new(0),
            object_size: 0,
        }
    }

    pub fn refill_from(
        &mut self,
        mh_offset: usize,
        mut bitmap: &mut Bitmap<AtomicBitmapBase<4>>,
    ) -> usize {
        if self.is_full() {
            0
        } else {
            let mut localbits = bitmap.set_and_exchange_all();

            let alloc_count = localbits
                .iter_mut()
                .map(|b| b.load(Ordering::SeqCst) as usize)
                .take_while(|b| *b <= N)
                .fold(0, |mut alloc_count, b| {
                    if self.is_full() {
                        bitmap.unset(b);
                    } else {
                        self.offset.inner().fetch_sub(1, Ordering::AcqRel);
                        unsafe {
                            self.array
                                .cast::<Entry>()
                                .add(self.offset.load(Ordering::Acquire) as usize)
                                .write(Entry::new(mh_offset, b));
                            alloc_count += 1;
                        }
                    }
                    alloc_count
                });

            alloc_count
        }
    }

    pub fn is_full(&self) -> bool {
        self.offset.load(Ordering::Release) == 0
    }

    pub fn is_exhausted(&self) -> bool {
        self.offset.load(Ordering::Acquire) as usize >= N
    }

    pub fn is_exhausted_and_no_refill(&mut self) -> bool {
        self.is_exhausted() && !self.local_refill()
    }

    pub fn insert(&mut self, heap: *mut MiniHeap) {
        let mini_heaps = unsafe { self.mini_heaps.as_mut_slice().as_mut().unwrap() };
        let mini_heap = mini_heaps.iter().position(|x| x.is_none());
        match mini_heap {
            Some(n) => mini_heaps[n] = Some(heap),
            None => unreachable!(),
        }
    }

    pub fn local_refill(&mut self) -> bool {
        let mut added_capacity = 0usize;

        let mini_heaps = unsafe { self.mini_heaps.as_mut_slice().as_mut().unwrap() };

        mini_heaps
            .iter()
            .position(|x| x.is_none())
            .map(|x| self.rng.shuffle(self.array as *mut [Entry; N], x, N))
            .is_some()
    }

    pub fn refill_mini_heaps(&mut self) {
        while self.offset.load(Ordering::Acquire) < N as u64 {
            let mut entry = self.pop();
            let mh = unsafe { self.mini_heaps.get(entry.mh_offset).unwrap() };
            unsafe {
                mh.as_mut()
                    .unwrap()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .free_offset(entry.bit_offset);
            }
        }
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
