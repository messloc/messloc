use std::sync::Mutex;

use once_cell::sync::Lazy;
use rand_mt::Mt19937GenRand64;
use std::ops::Range;

#[derive(Clone, Debug, Default)]
pub struct Mwc {
    inner: Mwc64,
}
impl Mwc {
    pub fn new(seed1: u64, seed2: u64) -> Self {
        Self {
            inner: Mwc64::new(seed1, seed2),
        }
    }
    // N.B.: this is specified with `ATTRIBUTE_ALWAYS_INLINE` in the C++ source.
    // not sure if this is a good idea, however
    #[inline(always)]
    pub fn in_range(&mut self, range: Range<usize>) -> usize {
        let spread = 1 + range.end - range.start;

        // adapted from https://lemire.me/blog/2016/06/27/a-fast-alternative-to-the-modulo-reduction/
        range.start + ((self.inner.next() as u32 as u64 * spread as u64) >> 32) as usize
    }
}

#[derive(Debug, Clone)]
pub struct Mwc64 {
    x: u64,
    c: u64,
    t: u64,
    value: u64,
    state: State,
}

const _: () = assert!(std::mem::size_of::<Mwc64>() == 40);

impl Mwc64 {
    pub fn new(seed1: u64, seed2: u64) -> Self {
        Self {
            x: seed1 << 32 + seed2,
            c: 123456_123456_123456u64,
            t: 0,
            value: 0,
            state: State::Regen,
        }
    }
    #[inline(always)]
    pub fn next(&mut self) -> u64 {
        match self.state {
            State::TopBytes => {
                self.state = State::BottomBytes;
                self.value >> 32
            }
            State::BottomBytes => {
                self.state = State::Regen;
                self.value & 0xffff_ffff
            }
            State::Regen => {
                self.value = self.update();
                self.state = State::TopBytes;
                // also return the top bytes
                self.value >> 32
            }
        }
    }
    fn update(&mut self) -> u64 {
        self.t = (self.x << 58) + self.c;
        self.c = self.x >> 6;
        self.x += self.t;
        if self.x < self.t {
            self.c += 1;
        }
        self.x
    }
}
impl Default for Mwc64 {
    fn default() -> Self {
        Self::new(seed(), seed())
    }
}

#[repr(u64)]
#[derive(Debug, Clone, Copy)]
enum State {
    TopBytes,
    BottomBytes,
    Regen,
}

static SEED_MUTEX: Lazy<Mutex<Mt19937GenRand64>> = Lazy::new(|| {
    let mut seed = [0u8; 8];
    let rand = getrandom::getrandom(&mut seed);
    let mt = Mt19937GenRand64::new(u64::from_be_bytes(seed));
    Mutex::new(mt)
});

#[inline]
fn seed() -> u64 {
    let mut lock = SEED_MUTEX.lock().unwrap();
    lock.next_u64()
}
