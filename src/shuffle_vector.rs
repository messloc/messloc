use std::mem::size_of;

use arrayvec::ArrayVec;

use crate::{consts::*, mini_heap::MiniHeap, rng::Mwc};

pub struct ShuffleVector {
    start: [usize; MAX_MINIHEAPS_PER_SHUFFLE_VECTOR],
    arena_begin: *const u8,
    max_count: u16,
    off: i16,
    object_size: u32,
    attached_mini_heaps: ArrayVec<MiniHeap, MAX_MINIHEAPS_PER_SHUFFLE_VECTOR>,
    prng: Mwc,
    object_size_reciprocal: f32,
    attached_off: u32,
    list: [Entry; MAX_SHUFFLE_VECTOR_LENGTH],
}
// const _: () = assert!(gcd(std::mem::size_of::<ShuffleVector>(), CACHELINE_SIZE) == CACHELINE_SIZE);

impl ShuffleVector {
    pub fn new() -> Self {
        Self {
            // TODO: hmm...
            start: Default::default(),
            arena_begin: std::ptr::null(),
            max_count: 0,
            off: 0,
            object_size: 0,
            attached_mini_heaps: Default::default(),
            prng: Mwc::default(),
            object_size_reciprocal: 0.0,
            attached_off: 0,
            list: [Default::default(); MAX_SHUFFLE_VECTOR_LENGTH],
        }
    }
    pub fn malloc(&mut self) -> *const () {
        // debug_assert!(!self.is_exhausted());
        let off = self.pop();
        ptr_from_offset(off)
    }
    pub fn free(&mut self, mh: *mut MiniHeap, ptr: *const ()) {
        
    }

    #[inline(always)]
    pub fn push(&mut self, entry: Entry) {
        // we must have at least 1 free space in the list
        debug_assert!(self.off > 0);

        self.off -= 1;
        self.list[self.off as usize] = entry;

        if ENABLE_SHUFFLE_ON_FREE {
            let range = self.off as usize..(self.max_count as usize - 1);
            let swap_off = self.prng.in_range(range);
            self.list.swap(self.off as usize, swap_off)
        }
    }
    #[inline(always)]
    pub fn pop(&mut self) -> Entry {
        debug_assert!(self.off >= 0);
        debug_assert!(self.off < self.max_count);

        let val = self.list[self.off as usize];
        self.off += 1;

        val
    }
}
impl Drop for ShuffleVector {
    fn drop(&mut self) {
        debug_assert!(self.attached_mini_heaps.is_empty())
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Entry {
    pub miniheap_offset: u8,
    pub bit_offset: u8,
}
const _: () = assert!(std::mem::size_of::<Entry>() == 2);

// adopted from https://docs.rs/num-integer/0.1.45/src/num_integer/lib.rs.html#855-879
const fn gcd(mut m: usize, mut n: usize) -> usize {
    if m == 0 || n == 0 {
        return m | n;
    }

    // find common factors of 2
    let shift = (m | n).trailing_zeros();

    // divide n and m by 2 until odd
    m >>= m.trailing_zeros();
    n >>= n.trailing_zeros();

    while m != n {
        if m > n {
            m -= n;
            m >>= m.trailing_zeros();
        } else {
            n -= m;
            n >>= n.trailing_zeros();
        }
    }
    m << shift
}
