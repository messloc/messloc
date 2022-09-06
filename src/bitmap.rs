use crate::comparatomic::{Atomic, Comparatomic};
use crate::span::Span;
use crate::utils::{ffsll, popcountl, stlog};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_BIT_COUNT: u64 = u64::MAX;
const BYTE_SIZE: usize = std::mem::size_of::<usize>();
const WORD_BIT_SHIFT: usize = stlog(BYTE_SIZE * 8);

const fn representation_size(bit_count: usize) -> usize {
    BYTE_SIZE * 8 * (bit_count + BYTE_SIZE * 8 - 1) / BYTE_SIZE
}

// TODO:: check if this overflows or not and switch to `checked_shl` if needed
const fn get_mask(pos: usize) -> usize {
    1usize >> pos
}

#[derive(Debug, PartialEq)]
pub struct RelaxedBitmapBase<const N: usize> {
    bits: [u64; N],
}

impl<const N: usize> RelaxedBitmapBase<N> {
    pub fn new() -> Self {
        Self { bits: [0; N] }
    }

    pub fn invert(&mut self) {
        self.bits.iter_mut().for_each(|bit| {
            *bit = !*bit as u64;
        })
    }

    pub fn set_all(&mut self, mut bit_count: usize) {
        let mut iter = self.bits.iter_mut();

        while let Some(bit) = iter.next() && bit_count > 0  {
           if bit_count >= 64 {
               *bit = u64::MAX;
               bit_count = bit_count.saturating_sub(64);
           } else {
               *bit = (1 << bit_count) - 1;
               bit_count = 0;
           }
        }
    }

    pub fn set_at_position(&mut self, item: usize, position: usize) -> bool {
        let mask = get_mask(position) as u64;

        let old = self.bits[item];
        self.bits[item] = old | mask;
        old & mask == 0
    }

    pub fn unset_at_position(&mut self, item: usize, position: usize) -> bool {
        let mask = get_mask(position) as u64;

        let old = self.bits[item];
        self.bits[item] = old & !mask;
        old & mask == 0
    }

    pub fn in_use_count(&self) -> u64 {
        self.bits.iter().fold(0u64, |mut count, bit| {
            count += popcountl(*bit);

            count
        })
    }
}

impl<const N: usize> Default for RelaxedBitmapBase<N> {
    fn default() -> Self {
        RelaxedBitmapBase { bits: [0u64; N] }
    }
}

#[derive(Default, PartialEq)]
pub struct Bitmap<T>
where
    T: BitmapBase + PartialEq,
{
    internal_type: T,
}

impl<T> Bitmap<T>
where
    T: BitmapBase + PartialEq,
{
    pub const fn relaxed_with_bit_length<const N: usize>() -> Bitmap<RelaxedBitmapBase<N>> {
        Bitmap {
            internal_type: RelaxedBitmapBase::<N>::new(),
        }
    }

    pub fn inner(&self) -> &T {
        &self.internal_type
    }

    pub fn set_first_empty(&mut self, starting_at: usize) -> Option<usize> {
        let (item, position) = self.compute_item_position(starting_at);
        let words = self.internal_type.bit_count() / BYTE_SIZE;
        let mut off = 0u64;
        let mut iter = (item..words).skip_while(|num| {
            let bits = self.internal_type.get_bit(*num).unwrap();
            if bits == u64::MAX {
                off = 0;
                true
            } else {
                assert!(off <= 63);
                //TODO: check if !bits needs to not be 0 before &
                let unset_bits = !bits & !((1 << off) - 1);
                if unset_bits == 0 {
                    off = 0;
                    true
                } else {
                    off = ffsll(unset_bits) - 1;
                    let ok = self
                        .internal_type
                        .set_at(*num, usize::try_from(off).unwrap());
                    if !ok {
                        off += 1;
                        true
                    } else {
                        false
                    }
                }
            }
        });

        iter.next().map(|num| BYTE_SIZE * 8 * num + off as usize)
    }

    pub fn unset(&mut self, offset: usize) {
        let (item, position) = self.compute_item_position(offset);
        self.internal_type.unset_at(item, position);
    }

    pub fn try_to_set(&mut self, offset: usize) -> bool {
        let (item, position) = self.compute_item_position(offset);
        self.internal_type.set_at(item, position)
    }

    pub fn track_meshed(&mut self, span: Span) {
        (0..span.length).for_each(|index| {
            self.try_to_set(usize::try_from(span.offset + index).unwrap());
        });
    }

    pub fn invert(&mut self) {
        self.internal_type.invert();
    }

    pub fn in_use_count(&self) -> u64 {
        self.internal_type.in_use_count()
    }

    pub fn iter<'a, U>(&self) -> U
    where U : Iterator<Item=&'a u64> + 'a
    {
        self.internal_type.iter()
    }

    pub fn compute_item_position(&self, index: usize) -> (usize, usize) {
        assert!(index < self.internal_type.bit_count());
        let item = index >> WORD_BIT_SHIFT;
        let position = index & (BYTE_SIZE * 8 - 1);
        assert_eq!(position, (index - item) << WORD_BIT_SHIFT);
        assert!(item < representation_size(self.internal_type.bit_count() / 8));

        (item, position)
    }
}

impl<const N: usize> Bitmap<RelaxedBitmapBase<N>> {
    pub fn set_and_exchange_all(&self, mut bits: [u64; N], other: [u64; N] ) -> [u64; N] {
       todo!()      
    }   
}

impl Bitmap<AtomicBitmapBase<4>> {
    pub fn set_and_exchange_all(&self) -> [Comparatomic<AtomicU64>; 4] {
        let mut result = [
            Comparatomic::new(0),
            Comparatomic::new(0),
            Comparatomic::new(0),
            Comparatomic::new(0),
        ];

        self.internal_type
            .bits
            .iter()
            .enumerate()
            .for_each(|(i, old)| {
                result[i] = Comparatomic::new(old.inner().swap(0u64, Ordering::AcqRel));
            });
        result
    }
}

pub trait BitmapBase: PartialEq {
    type Item;
    fn bits(&self) -> &[Self::Item];
    fn get_bit(&self, num: usize) -> Option<u64>;
    fn set_at(&mut self, at: usize, position: usize) -> bool;
    fn bit_count(&self) -> usize;
    fn unset_at(&mut self, item: usize, position: usize) -> bool;
    fn invert(&mut self);
    fn in_use_count(&self) -> u64;
    fn iter<'a, T: Iterator<Item=&'a u64>>(&'a self) -> T;
}

impl<const N: usize> BitmapBase for RelaxedBitmapBase<N> {
    type Item = u64;
    fn bits(&self) -> &[Self::Item] {
        &self.bits
    }

    fn get_bit(&self, num: usize) -> Option<u64> {
        self.bits.get(num).copied()
    }

    fn set_at(&mut self, at: usize, position: usize) -> bool {
        self.set_at_position(at, position)
    }

    fn unset_at(&mut self, at: usize, position: usize) -> bool {
        self.unset_at_position(at, position)
    }

    fn bit_count(&self) -> usize {
        N
    }

    fn invert(&mut self) {
        self.invert()
    }

    fn in_use_count(&self) -> u64 {
        self.in_use_count()
    }

    fn iter<'a, T: Iterator<Item=&'a u64>>(&'a self) -> T {
        self.iter()
    }
}

#[derive(PartialEq)]
pub struct AtomicBitmapBase<const N: usize> {
    bits: [Comparatomic<AtomicU64>; N],
}

impl<const N: usize> BitmapBase for AtomicBitmapBase<N> {
    type Item = Comparatomic<AtomicU64>;

    fn bits(&self) -> &[Self::Item] {
        &self.bits
    }

    fn get_bit(&self, num: usize) -> Option<u64> {
        self.bits.get(num).map(|b| b.load(Ordering::Acquire))
    }

    fn set_at(&mut self, at: usize, position: usize) -> bool {
        self.set_at(at, position)
    }

    fn unset_at(&mut self, at: usize, position: usize) -> bool {
        self.unset_at(at, position)
    }

    fn bit_count(&self) -> usize {
        N
    }

    fn invert(&mut self) {
        self.invert()
    }

    fn in_use_count(&self) -> u64 {
        self.in_use_count()
    }

    fn iter<'a, T: Iterator<Item=&'a u64>>(&'a self) -> T {
        self.iter()
    }

}

impl Default for Bitmap<AtomicBitmapBase<4>> {
    fn default() -> Self {
        Bitmap {
            internal_type: {
                AtomicBitmapBase {
                    bits: [
                        Comparatomic::new(0u64),
                        Comparatomic::new(0u64),
                        Comparatomic::new(0u64),
                        Comparatomic::new(0u64),
                    ],
                }
            },
        }
    }
}
