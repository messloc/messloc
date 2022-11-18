use std::cell::RefCell;
use std::num::{NonZeroU64, NonZeroUsize};
use std::ops::{Range, RangeFrom, RangeFull, RangeInclusive};
use std::sync::Arc;
use std::sync::Mutex;

use rand_xoshiro::rand_core::{RngCore, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

pub struct Rng {
    rng: Arc<Mutex<Xoshiro256PlusPlus>>,
}

impl Rng {
    pub fn init() -> Self {
        Self {
            rng: Arc::new(Mutex::new(Xoshiro256PlusPlus::seed_from_u64(0))),
        }
    }

    pub fn shuffle<T>(&self, data: &mut [T], mut start: usize, mut end: usize) {
        while start != end {
            let diff = end - start;
            if diff >= 1 {
                let st = start;
                start += 1;
                let item = self.in_range(start..=end);
                data.swap(start, end);
                end -= 1;
            } else {
                break;
            }
        }
    }

    pub fn in_range(&self, range: RangeInclusive<usize>) -> usize {
        let range = 1 + range.end() - range.start();
        (usize::try_from(self.next()).unwrap() * range) >> 32
    }

    pub fn next(&self) -> u64 {
        self.rng.lock().unwrap().next_u64()
    }
}
