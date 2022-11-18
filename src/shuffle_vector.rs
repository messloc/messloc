use std::cell::RefCell;
use std::ops::DerefMut;
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use crate::list_entry::ListEntry;
use crate::{
    bitmap::{AtomicBitmapBase, Bitmap},
    comparatomic::Comparatomic,
    mini_heap::MiniHeap,
    rng::Rng,
};
use crate::{ENABLED_SHUFFLE_ON_INIT, MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR};

pub struct ShuffleVector<const N: usize> {
    start: [Comparatomic<AtomicPtr<MiniHeap>>; MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR],
    array: RefCell<[Entry; N]>,
    pub mini_heaps: Vec<*mut MiniHeap>,
    rng: Rng,
    offset: Comparatomic<AtomicU64>,
    attached_offset: Comparatomic<AtomicU64>,
    object_size: usize,
}
impl<const N: usize> ShuffleVector<N> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            start: std::array::from_fn(|_| Comparatomic::new(null_mut())),
            array: RefCell::new(std::array::from_fn(|_| Entry::new(0, 0))),
            mini_heaps: vec![],
            rng: Rng::init(),
            offset: Comparatomic::new(0),
            attached_offset: Comparatomic::new(0),
            object_size: 0,
        }
    }

    pub fn refill_from(
        &self,
        mh_offset: usize,
        bitmap: Rc<RefCell<Bitmap<AtomicBitmapBase<4>>>>,
    ) -> usize {
        if self.is_full() {
            0
        } else {
            let mut localbits = bitmap.borrow_mut().set_and_exchange_all();

            let alloc_count = localbits
                .iter()
                .map(|b| b.load(Ordering::SeqCst) as usize)
                .take_while(|b| *b <= N)
                .fold(0, |mut alloc_count, b| {
                    if self.is_full() {
                        bitmap.borrow_mut().unset(b);
                    } else {
                        self.offset.inner().fetch_sub(1, Ordering::AcqRel);
                        self.array.borrow_mut()[self.offset.load(Ordering::Acquire) as usize] =
                            Entry::new(mh_offset, b);
                        alloc_count += 1;
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
        self.offset.load(Ordering::Release) as usize >= N
    }

    pub fn is_exhausted_and_no_refill(&self) -> bool {
        self.is_exhausted() && !self.local_refill()
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
                self.rng.shuffle(
                    self.array.borrow_mut().deref_mut(),
                    self.offset.load(Ordering::Release) as usize,
                    N,
                );
            }
            true
        } else {
            false
        }
    }

    pub fn refill_mini_heaps(&mut self) {
        while self.offset.load(Ordering::Release) < N as u64 {
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

    pub fn malloc(&self) -> *mut MiniHeap {
        assert!(!self.is_exhausted());
        let entry = self.pop().unwrap();

        self.ptr_from_offset(&entry)
    }

    pub fn as_cloned_vector(&self) -> Vec<*mut MiniHeap> {
        self.mini_heaps.clone()
    }

    pub fn pop(&self) -> Option<Entry> {
        let val = self
            .array
            .borrow()
            .get(self.offset.load(Ordering::AcqRel) as usize)
            .cloned();
        self.offset.fetch_add(1, Ordering::AcqRel);
        val
    }

    pub fn re_init(&mut self) {
        self.offset.store(N as u64, Ordering::Acquire);
        self.attached_offset.store(0, Ordering::AcqRel);
        let len = self.mini_heaps.len();
        self.rng.shuffle(&mut self.mini_heaps, 0, len);

        self.mini_heaps.iter().enumerate().for_each(|(i, mut mh)| {
            let mh = unsafe { mh.as_mut().unwrap() };
            self.start[i] = unsafe { Comparatomic::new(mh.span_start) };
            mh.set_sv_offset(i);
        });
    }

    pub fn ptr_from_offset(&self, offset: &Entry) -> *mut MiniHeap {
        assert!(offset.mh_offset < self.mini_heaps.len());

        unsafe {
            self.start[offset.mh_offset]
                .load(Ordering::AcqRel)
                .add(offset.bit_offset * self.object_size)
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
