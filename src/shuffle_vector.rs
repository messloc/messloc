use core::ops::DerefMut;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use crate::list_entry::ListEntry;
use crate::one_way_mmap_heap::{Heap, OneWayMmapHeap};
use crate::{
    bitmap::{AtomicBitmapBase, Bitmap},
    comparatomic::Comparatomic,
    mini_heap::MiniHeap,
    rng::Rng,
};
use crate::{ENABLED_SHUFFLE_ON_INIT, MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR};

pub struct ShuffleVector<const N: usize> {
    pub start: *mut (),
    array: *mut (),
    pub mini_heaps: *mut *mut (),
    pub mini_heap_count: usize,
    rng: Rng,
    pub offset: Comparatomic<AtomicU64>,
    attached_offset: Comparatomic<AtomicU64>,
    object_size: usize,
}
impl<const N: usize> ShuffleVector<N> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mini_heaps = unsafe {
            OneWayMmapHeap
                .malloc(core::mem::size_of::<MiniHeap>() * MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR)
        } as *mut MiniHeap;

        Self {
            start: null_mut(),
            array: null_mut(),
            mini_heaps: mini_heaps.cast(),
            mini_heap_count: 0,
            rng: Rng::init(),
            offset: Comparatomic::new(0),
            attached_offset: Comparatomic::new(0),
            object_size: 0,
        }
    }

    pub fn from_allocated(alloc: *mut *mut (), bytes: usize) -> Self {
        let array =
            unsafe { OneWayMmapHeap.malloc(core::mem::size_of::<Entry>() * N) } as *mut Entry;

        unsafe { array.write(Entry::new(0, bytes)) };

        ShuffleVector {
            start: alloc.cast(),
            array: array as *mut (),
            mini_heaps: alloc,
            mini_heap_count: 0,
            rng: Rng::init(),
            offset: Comparatomic::new(1),
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
        let mini_heaps = unsafe {
            core::ptr::slice_from_raw_parts_mut(
                self.mini_heaps.cast::<*mut MiniHeap>(),
                MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR,
            )
            .as_mut()
            .unwrap()
        };

        match mini_heaps.iter_mut().enumerate().find(|(i, h)| h.is_null()) {
            Some((i, h)) => {
                *h = heap;
            }
            None => {
                unreachable!()
            }
        };
        self.offset.fetch_add(1, Ordering::AcqRel);
    }

    pub fn local_refill(&mut self) -> bool {
        dbg!("here");
        let mut added_capacity = 0usize;

        if self.mini_heaps.is_null() {
            dbg!("here");
            false
        } else {
            loop {
                let offset = self.attached_offset.load(Ordering::Acquire) as usize;

                let mut mh = unsafe {
                    let heap = self
                        .mini_heaps
                        .cast::<[*mut MiniHeap; MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR]>()
                        .add(offset) as *mut MiniHeap;
                    heap.as_mut().unwrap()
                };

                if mh.is_full() {
                    continue;
                }

                let alloc_count = self.refill_from(
                    self.attached_offset.load(Ordering::Acquire) as usize,
                    &mut mh.bitmap,
                );
                added_capacity |= alloc_count;
                self.attached_offset.fetch_add(1, Ordering::AcqRel);
            }

            if added_capacity > 0 {
                if ENABLED_SHUFFLE_ON_INIT {
                    self.rng.shuffle(
                        self.array as *mut [Entry; N],
                        self.offset.load(Ordering::Acquire) as usize,
                        N,
                    );
                }
                true
            } else {
                false
            }
        }
    }

    pub fn refill_mini_heaps(&mut self) {
        while self.offset.load(Ordering::Acquire) < N as u64 {
            let mut entry = self.pop();
            let mut mh = unsafe {
                self.mini_heaps
                    .cast::<[*mut MiniHeap; MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR]>()
                    .add(entry.mh_offset) as *mut MiniHeap
            };

            unsafe {
                mh.as_mut().unwrap().free_offset(entry.bit_offset);
            }
        }
    }

    pub fn malloc(&self) -> *mut MiniHeap {
        if !self.array.is_null() {
            let entry = self.pop();
            dbg!("yee");
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

    pub fn re_init(&mut self) {
        self.offset.store(N as u64, Ordering::Acquire);
        self.attached_offset.store(0, Ordering::AcqRel);
        let mini_heaps = unsafe {
            self.mini_heaps
                .cast::<[*mut MiniHeap; MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR]>()
                .as_mut()
                .unwrap()
        };
        let len = mini_heaps.len();
        self.rng.shuffle(mini_heaps, 0, len);

        mini_heaps.iter().enumerate().for_each(|(i, mut mh)| {
            let mh = unsafe { mh.as_mut().unwrap() };
            mh.set_sv_offset(i);
        });
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
    pub const fn new(mh_offset: usize, bit_offset: usize) -> Self {
        Self {
            mh_offset,
            bit_offset,
        }
    }
}
