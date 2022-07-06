use std::{
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering}, fmt::Display,
};

pub type AtomicBitmap = AtomicBitmapBase<256>;

pub trait Bitmap {
    type Word;

    fn bit_count(&self) -> usize;
    fn set_at(&mut self, item: u32, pos: u32) -> bool;
    fn unset_at(&mut self, item: u32, pos: u32) -> bool;
    fn in_use_count(&self) -> u32;

    fn byte_count(&self) -> usize {
        repr_size(self.bit_count())
    }
}

// it's a bit of a shame that we can't do the same level of `constexpr`
// magic as the C++ implementation.
const WORD_COUNT: usize = 4;
pub struct AtomicBitmapBase<const MAX_BITS: usize> {
    bits: [AtomicUsize; WORD_COUNT],
}
impl<const MAX_BITS: usize> AtomicBitmapBase<MAX_BITS> {
    pub fn new(bit_count: usize) -> Self {
        debug_assert!(
            bit_count <= MAX_BITS,
            "max bits ({MAX_BITS}) exceeded: {bit_count}"
        );

        let bits = [BIT_INIT; WORD_COUNT];
        Self { bits }
    }
    fn atomic_set_at(&mut self, item: u32, pos: u32) -> bool {
        let mask = 1 << pos;
        let bit = &self.bits[item as usize];
        let old = bit.load(Ordering::Relaxed);
        // spinloop
        while bit
            .compare_exchange_weak(old, old | mask, Ordering::Release, Ordering::Relaxed)
            .is_err()
        {}
        old & mask == 0
    }

    fn atomic_unset_at(&mut self, item: u32, pos: u32) -> bool {
        let mask = 1 << pos;
        let bit = &self.bits[item as usize];
        let old = bit.load(Ordering::Relaxed);
        // spinloop
        while bit
            .compare_exchange_weak(old, old & !mask, Ordering::Release, Ordering::Relaxed)
            .is_err()
        {}
        old & mask == 0
    }
    fn set_and_exchange_all(
        &self,
        old_bits: &mut [usize; WORD_COUNT],
        new_bits: &[usize; WORD_COUNT],
    ) {
        for i in 0..WORD_COUNT {
            old_bits[i] = self.bits[i].swap(new_bits[i], Ordering::AcqRel);
        }
    }
}
impl<const MAX_BITS: usize> Bitmap for AtomicBitmapBase<MAX_BITS> {
    type Word = AtomicUsize;

    fn bit_count(&self) -> usize {
        MAX_BITS
    }
    fn set_at(&mut self, item: u32, pos: u32) -> bool {
        self.atomic_set_at(item, pos)
    }

    fn unset_at(&mut self, item: u32, pos: u32) -> bool {
        self.atomic_unset_at(item, pos)
    }
    fn in_use_count(&self) -> u32 {
        self.bits
            .iter()
            .map(|b| b.load(Ordering::Relaxed).count_ones())
            .sum()
    }
}

pub struct RelaxedBitmapBase {
    count: RelaxedCount,
    // could be on the heap or borrowed, thus we can't have a meaningful lifetime here
    bits: NonNull<usize>,
}
impl RelaxedBitmapBase {
    fn new(bit_count: usize) -> Self {
        let count = RelaxedCount::new(bit_count, true);
        let bits = todo!("allocate memory");
        let mut this = Self { count, bits };
        this.clear();
        this
    }
    /// # Safety
    /// Caller must ensure that `memory` must be well-aligned and [valid](std::ptr#safety)
    /// for reads and writes of up to `repr_size(bit_count)` bytes.
    unsafe fn with_backing_memory(bit_count: usize, memory: NonNull<u8>, clear: bool) -> Self {
        let count = RelaxedCount::new(bit_count, false);
        let mut this = Self {
            count,
            bits: memory.cast(),
        };
        if clear {
            this.clear();
        }
        this
    }

    fn clear(&mut self) {
        unsafe {
            // SAFETY: with or without dynamic allocation, the `bits` pointer
            // is guaranteed to be properly aligned and valid for writes of
            // `repr_size(bit_count)` bytes.

            // FIXME(leocth): is there a less cursed way of writing this?
            std::ptr::write_bytes(self.bits.as_ptr(), 0, word_count(self.count.value()))
        }
    }
}
impl Bitmap for RelaxedBitmapBase {
    type Word = usize;

    fn bit_count(&self) -> usize {
        self.count.value()
    }
    fn set_at(&mut self, item: u32, pos: u32) -> bool {
        assert!(
            item < word_count(self.count.value()) as u32,
            "item index out of bounds"
        );

        let mask = 1 << pos;
        unsafe {
            // SAFETY: `self.bits` is valid for reads and writes up to
            // `repr_size(bit_count)`, which is greater than `item`.
            let ptr = self.bits.as_ptr().add(item as usize);
            let old = *ptr;
            *ptr = old & !mask;
            old & mask == 0
        }
    }
    fn unset_at(&mut self, item: u32, pos: u32) -> bool {
        assert!(
            item < word_count(self.count.value()) as u32,
            "item index out of bounds"
        );

        let mask = 1 << pos;
        unsafe {
            // SAFETY: `self.bits` is valid for reads and writes up to
            // `repr_size(bit_count)`, which is greater than `item`.
            let ptr = self.bits.as_ptr().add(item as usize);
            let old = *ptr;
            *ptr = old & !mask;
            old & mask == 0
        }
    }
    fn in_use_count(&self) -> u32 {
        let word_count = word_count(self.count.value());

        let mut count = 0;
        for i in 0..word_count {
            count += unsafe {
                // SAFETY: `self.bits` is valid for reads and writes up to
                // `repr_size(bit_count)` bytes, or `word_count` usizes.
                let ptr = self.bits.as_ptr().add(i);
                (*ptr).count_ones()
            }
        }
        count
    }
}
impl Drop for RelaxedBitmapBase {
    fn drop(&mut self) {
        if self.count.is_dynamically_allocated() {
            todo!("deallocate memory");
        }
    }
}

#[derive(Clone, Copy)]
struct RelaxedCount(u64);
impl RelaxedCount {
    fn new(count: usize, dynamically_allocated: bool) -> Self {
        let mut inner = (count as u64) << 1;
        if dynamically_allocated {
            inner |= 1;
        }
        Self(inner)
    }
    fn value(self) -> usize {
        (self.0 >> 1) as usize
    }
    fn is_dynamically_allocated(self) -> bool {
        self.0 & 1 != 0
    }
}

const BIT_INIT: AtomicUsize = AtomicUsize::new(0);

const WORD_BYTES: usize = std::mem::size_of::<usize>();
const WORD_BITS: usize = WORD_BYTES * 8;

const fn repr_size(bit_count: usize) -> usize {
    WORD_BITS * ((bit_count + WORD_BITS - 1) / WORD_BITS) / 8
}
const fn word_count(bit_count: usize) -> usize {
    repr_size(bit_count) / WORD_BYTES
}
