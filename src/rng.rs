use std::num::{NonZeroU64, NonZeroUsize};
use std::ops::{Range, RangeFrom, RangeFull, RangeInclusive};

pub struct Rng {
    z: NonZeroUsize,
    w: NonZeroUsize,
}

impl Rng {
    pub fn new(seed1: usize, seed2: usize) -> Rng {
        Rng {
            z: NonZeroUsize::new(seed1).unwrap(),
            w: NonZeroUsize::new(seed2).unwrap(),
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
        (self.next() * range) >> 32
    }

    pub fn next(&mut self) -> usize {
        let mut z = self.z.get();
        let mut w = self.w.get();
        z = 36_969 * (z & 65_535) + (z >> 16);
        w = 18_000 * (w & 65_535) + (w >> 16);
        let x = z << 16 + w;
        self.z = NonZeroUsize::new(z).unwrap();
        self.w = NonZeroUsize::new(w).unwrap();
        x
    }
}
