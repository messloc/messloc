use crate::comparatomic::Comparatomic;
use std::sync::atomic::{
    AtomicU64,
    Ordering::{AcqRel, Release},
};

#[derive(Default, PartialEq)]
pub struct AtomicBitmap256 {
    // TODO: support non u64 atomic platforms?
    bits: [Comparatomic<AtomicU64>; 4],
}

impl AtomicBitmap256 {
    pub fn exchange(&self, from: AtomicBitmap256) -> AtomicBitmap256 {
        let AtomicBitmap256 {
            bits: [s0, s1, s2, s3],
        } = self;
        let AtomicBitmap256 {
            bits: [f0, f1, f2, f3],
        } = from;
        AtomicBitmap256 {
            bits: [
                Comparatomic::new(s0.inner().swap(f0.into_inner().into_inner(), AcqRel)),
                Comparatomic::new(s1.inner().swap(f1.into_inner().into_inner(), AcqRel)),
                Comparatomic::new(s2.inner().swap(f2.into_inner().into_inner(), AcqRel)),
                Comparatomic::new(s3.inner().swap(f3.into_inner().into_inner(), AcqRel)),
            ],
        }
    }

    fn set_at(&self, item: u32, pos: u32) -> bool {
        let mask: u64 = 1 << pos;
        let old_value = self.bits[item as usize].inner().fetch_or(mask, Release);
        (old_value & mask) == 0
    }

    fn unset_at(&self, item: u32, pos: u32) -> bool {
        let mask: u64 = 1 << pos;
        let old_value = self.bits[item as usize].inner().fetch_and(!mask, Release);
        (old_value & mask) == 0
    }

    pub fn try_to_set(&self, index: u64) -> bool {
        let (item, position) = Self::compute_item_position(index);
        self.set_at(item, position)
    }

    fn compute_item_position(index: u64) -> (u32, u32) {
        let item = index >> 6;
        let position = index & (64 - 1);
        (item as u32, position as u32)
    }
}
