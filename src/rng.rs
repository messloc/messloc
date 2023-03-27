
use rand_xoshiro::rand_core::{RngCore, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

pub struct Rng {
    rng: Xoshiro256PlusPlus,
}

impl Rng {
    pub fn init() -> Self {
        Self {
            rng: Xoshiro256PlusPlus::seed_from_u64(0),
        }
    }

    //TODO:: rewriting shuffling logic
    pub fn shuffle<T, const N: usize>(
        &mut self,
        data: *mut [T; N],
        start: usize,
        end: usize,
    ) {
        unsafe {
            let mut start = data.add(start);
            let mut end = start.add(end);

            while start != end {
                start = start.add(1);
                let diff = end.offset_from(start);
                if diff > 0 {
                    let item = self.in_range(diff as usize);
                    core::ptr::swap(start, end);
                    end = end.sub(1);
                } else {
                    break;
                }
            }
        }
    }

    pub fn in_range(&mut self, end: usize) -> usize {
        (usize::try_from(self.next()).unwrap() * end) >> 32
    }

    pub fn next(&mut self) -> u64 {
        self.rng.next_u64()
    }
}
