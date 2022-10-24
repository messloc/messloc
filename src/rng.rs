use std::cell::RefCell;
use std::num::{NonZeroU64, NonZeroUsize};
use std::ops::{Range, RangeFrom, RangeFull, RangeInclusive};
use std::rc::Rc;

use rand_xoshiro::rand_core::{RngCore, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

pub struct Rng {
    rng: Rc<RefCell<Xoshiro256PlusPlus>>,
}

impl Rng {
    pub fn init() -> Rng {
        Rng {
            rng: Rc::new(RefCell::new(Xoshiro256PlusPlus::seed_from_u64(0))),
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
        self.rng.borrow_mut().next_u64()
    }
}
