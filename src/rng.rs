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

    pub fn in_range(&mut self, start: usize, end: usize) -> usize {
        let range = 1 + end - start;
        (usize::try_from(self.next()).unwrap() * range) >> 32
    }

    pub fn next(&mut self) -> u64 {
        self.rng.next_u64()
    }
}
