use std::cell::RefCell;
use std::ops::DerefMut;
use std::rc::Rc;
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use crate::{
    bitmap::{AtomicBitmapBase, Bitmap},
    comparatomic::Comparatomic,
    mini_heap::MiniHeap,
    rng::Rng,
};
use crate::{ENABLED_SHUFFLE_ON_INIT, MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR};

pub struct ShuffleVector<'a, const N: usize> {
    start: [Comparatomic<AtomicPtr<MiniHeap<'a>>>; MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR],
    array: RefCell<[Entry; N]>,
    pub mini_heaps: Vec<*mut MiniHeap<'a>>,
    rng: Rng,
    offset: usize,
    attached_offset: Comparatomic<AtomicU64>,
    object_size: usize,
}

impl<'a, const N: usize> ShuffleVector<'a, N> {
    pub fn refill_from(
        &self,
        mh_offset: usize,
        bitmap: Rc<RefCell<Bitmap<AtomicBitmapBase<4>>>>,
    ) -> usize {
        if !self.is_full() {
            let mut localbits = bitmap.borrow_mut().set_and_exchange_all();

            let alloc_count = localbits
                .iter()
                .map(|b| b.load(Ordering::SeqCst) as usize)
                .take_while(|b| *b <= N)
                .fold(0, |mut alloc_count, b| {
                    if self.is_full() {
                        bitmap.borrow_mut().unset(b);
                    } else {
                        self.offset.saturating_sub(1);
                        self.array.borrow_mut()[self.offset] = Entry::new(mh_offset, b);
                        alloc_count += 1;
                    }
                    alloc_count
                });

            alloc_count
        } else {
            0
        }
    }

    pub fn is_full(&self) -> bool {
        self.offset == 0
    }

    pub fn is_exhausted(&self) -> bool {
        self.offset >= N
    }

    pub fn local_refill(&self) -> bool {
        let mut added_capacity = 0usize;
        loop {
            if self.attached_offset.load(Ordering::AcqRel) >= self.mini_heaps.len() as u64 {
                self.attached_offset.store(0, Ordering::AcqRel);
            }

            let mut mh = unsafe {
                self.mini_heaps
                    .get(self.attached_offset.load(Ordering::AcqRel) as usize)
                    .unwrap()
                    .as_mut()
                    .unwrap()
            };
            if mh.is_full() {
                continue;
            }

            let alloc_count = self.refill_from(
                self.attached_offset.load(Ordering::AcqRel) as usize,
                mh.bitmap(),
            );
            added_capacity |= alloc_count;
            self.attached_offset.fetch_add(1, Ordering::AcqRel);
        }

        if added_capacity > 0 {
            if ENABLED_SHUFFLE_ON_INIT {
                self.rng
                    .shuffle(self.array.borrow_mut().deref_mut(), self.offset, N);
            }
            true
        } else {
            false
        }
    }

    pub fn refill_mini_heaps(&mut self) {
        while self.offset < N {
            let mut entry = self.pop().unwrap();
            unsafe {
                self.mini_heaps
                    .get(entry.mh_offset)
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .free_offset(entry.bit_offset);
            }
        }
    }

    pub fn malloc(&mut self) -> *mut MiniHeap<'a> {
        assert!(!self.is_exhausted());
        let entry = self.pop().unwrap();

        self.ptr_from_offset(entry)
    }

    pub fn pop(&mut self) -> Option<Entry> {
        let val = self.array.borrow().get(self.offset).cloned();
        self.offset += 1;
        val
    }

    pub fn re_init(&mut self) {
        self.offset = N;
        self.attached_offset.store(0, Ordering::AcqRel);
        let len = self.mini_heaps.len();
        self.rng.shuffle(&mut self.mini_heaps, 0, len);

        self.mini_heaps.iter().enumerate().for_each(|(i, mut mh)| {
            let mh = unsafe { mh.as_mut().unwrap() };
            self.start[i] = unsafe { Comparatomic::new(mh.span_start) };
            mh.set_sv_offset(i);
        });
    }

    pub fn ptr_from_offset(&self, offset: Entry) -> *mut MiniHeap<'a> {
        assert!(offset.mh_offset < self.mini_heaps.len());

        unsafe {
            self.start[offset.mh_offset]
                .load(Ordering::AcqRel)
                .add(offset.bit_offset * self.object_size)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Entry {
    mh_offset: usize,
    bit_offset: usize,
}

impl Entry {
    pub fn new(mh_offset: usize, bit_offset: usize) -> Self {
        Entry {
            mh_offset,
            bit_offset,
        }
    }
}
