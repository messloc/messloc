use std::{marker::PhantomData, sync::atomic::AtomicPtr};
use std::sync::atomic::Ordering;
use arrayvec::ArrayVec;
use crate::{rng::Rng, bitmap::{Bitmap, BitmapBase, RelaxedBitmapBase, AtomicBitmapBase}, mini_heap::MiniHeap, comparatomic::Comparatomic};
use crate::{MAX_SHUFFLE_VECTOR_LENGTH, ENABLED_SHUFFLE_ON_INIT, MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR};

pub struct ShuffleVector<'a, const N: usize> {
    start: [Comparatomic<AtomicPtr<()>>;MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR],
    array: [Entry; N],
    mini_heaps: ArrayVec<&'a MiniHeap<'a>, N>,
    rng: Rng,
    offset: usize,
    attached_offset: usize,
    object_size: usize,
}

impl<const N: usize> ShuffleVector<'_, N> {

    pub fn refill_from(&mut self, mh_offset: usize, bitmap: &Bitmap<AtomicBitmapBase<4>>) -> usize {

       if !self.is_full() {
          let mut localbits = bitmap.set_and_exchange_all();

          //TODO:: maybe we dont need this condition
          let alloc_count = localbits.iter().map(|b| b.load(Ordering::SeqCst) as usize).take_while(|b| *b <= N).fold(0, |mut alloc_count, b| {
            if self.is_full() {
              bitmap.unset(b);
            } else {
                self.offset.saturating_sub(1);
                self.array[self.offset] = Entry::new(mh_offset, b);
                alloc_count+= 1;
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

    pub fn mini_heaps(&self) -> &ArrayVec<&'_ MiniHeap<'_>, N> {
        &self.mini_heaps
    }

    pub fn mini_heaps_mut(&mut self) -> &mut ArrayVec<&'_ MiniHeap<'_>, N> {
        &mut self.mini_heaps
    }

    pub fn local_refill(&mut self) -> bool {
        let added_capacity = 0usize; 
        loop {
            if self.attached_offset >= self.mini_heaps.len() {
                self.attached_offset = 0;
            }

            let mh = self.mini_heaps.get(self.attached_offset).unwrap();
            if mh.is_full() {
                continue;
            }

            let alloc_count = self.refill_from(self.attached_offset, mh.bitmap_mut());
            added_capacity |= alloc_count;
            self.attached_offset += 1;


        }

        if added_capacity > 0 {
            if ENABLED_SHUFFLE_ON_INIT {
                self.rng.shuffle(&mut self.array, self.offset, N);
            }
            true
        } else {
            false
        }
    }

    pub fn refill_mini_heaps(&self) {
        while self.offset < N {
            let entry = self.pop().unwrap();
            self.mini_heaps.get(entry.mh_offset).unwrap().free_offset(entry.bit_offset);
        }
    }

    pub fn malloc(&self) -> *mut () {
        assert!(!self.is_exhausted());
        self.ptr_from_offset(self.pop().unwrap())
    }

    pub fn pop(&mut self) -> Option<Entry> {
        let val = self.array.get(self.offset)?;
        self.offset += 1;
        Some(*val)
    }

    pub fn re_init(&mut self) {
        self.offset = N;
        self.attached_offset = 0;
        self.rng.shuffle(&mut self.mini_heaps, 0, self.mini_heaps.len());

        self.mini_heaps.iter().enumerate().for_each(|(i, mh)| {
            self.start[i] = unsafe { Comparatomic::new(mh.get_span_start() as *mut ()) };
            mh.set_sv_offset(i);
            assert!(mh.is_attached());
        });
    }

    pub fn ptr_from_offset(&self, offset: Entry) -> *mut () {
        assert!(offset.mh_offset < self.mini_heaps.len());

        unsafe { self.start[offset.mh_offset].load(Ordering::AcqRel).add(offset.bit_offset * self.object_size) }
    }

}

pub struct Entry {
    mh_offset: usize,
    bit_offset: usize
}

impl Entry {
    pub fn new(mh_offset: usize, bit_offset: usize) -> Self {
        Entry {
            mh_offset,
            bit_offset
        }
    }
}
