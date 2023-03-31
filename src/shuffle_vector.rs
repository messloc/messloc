use crate::MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR;
use crate::{fake_std::dynarray::DynDeq, mini_heap::MiniHeap, rng::Rng};
use core::ptr::null_mut;

pub struct ShuffleVector<const N: usize> {
    pub mini_heaps: DynDeq<MiniHeap, MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR>,
    rng: Rng,
}

impl<const N: usize> ShuffleVector<N> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            mini_heaps: DynDeq::create(),
            rng: Rng::init(),
        }
    }

    pub fn malloc(&mut self) -> *mut MiniHeap {
        match self.mini_heaps.pop() {
                Some(Some(v)) if let capacity = self.mini_heaps.capacity() && capacity >  4 => {
                    self.shuffle(0, capacity);
                    v
                },
                Some(Some(v)) => v,
                _ => null_mut()

            }
    }

    pub fn insert(&self, value: *mut MiniHeap) {
        if self.mini_heaps.push(value).is_none() {
            todo!()
        } else {
            self.mini_heaps.push(value);
        }
    }

    pub fn shuffle(&mut self, start: usize, end: usize) {
        (start..=end).rev().enumerate().for_each(|(k, _)| {
            let random = self.rng.in_range(start, k);
            self.mini_heaps.swap_indices(k, random);
        });
    }
}
