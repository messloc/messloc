use crate::class_array::CLASS_ARRAY;
use crate::comparatomic::Comparatomic;
use crate::MAX_SMALL_SIZE;
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(PartialEq, Default)]
pub struct Flags {
    flags: Comparatomic<AtomicU32>,
}

impl Flags {
    const SIZE_CLASS_SHIFT: u32 = 0;
    const FREELIST_ID_SHIFT: u32 = 6;
    const SHUFFLE_VECTOR_OFFSET_SHIFT: u32 = 8;
    const MAX_COUNT_SHIFT: u32 = 16;
    pub const MESHED_OFFSET: u32 = 30;
    pub fn new(max_count: u32, size_class: u32, sv_offset: u32, freelist_id: u32) -> Self {
        let flags = (max_count << Self::MAX_COUNT_SHIFT)
            + (size_class << Self::SIZE_CLASS_SHIFT)
            + (sv_offset << Self::SHUFFLE_VECTOR_OFFSET_SHIFT)
            + (freelist_id << Self::FREELIST_ID_SHIFT);
        Self {
            flags: Comparatomic::new(flags),
        }
    }

    pub fn set(&self, offset: u32) {
        let mask = 1u32.checked_shl(offset).unwrap();
        let old_flags = self.flags.load(Ordering::Acquire);
        loop {
            if (self
                .flags
                .inner()
                .compare_exchange_weak(
                    old_flags,
                    old_flags | mask,
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_err())
            {
                break;
            }
        }
    }

    fn set_at(&self, pos: u32) {
        let mask: u32 = 1 << pos;
        let old_value = self.flags.inner().fetch_or(mask, Ordering::Release);
    }

    fn unset_at(&self, pos: u32) {
        let mask: u32 = 1 << pos;
        let old_value = self.flags.inner().fetch_and(!mask, Ordering::Release);
    }

    fn set_masked(&self, mask: u32, new_val: u32) {
        self.flags
            .inner()
            .fetch_update(Ordering::Release, Ordering::Relaxed, |old| {
                Some((old & mask) | new_val)
            })
            .unwrap();
    }

    pub fn max_count(&self) -> u32 {
        (self.flags.inner().load(Ordering::SeqCst) >> Self::MAX_COUNT_SHIFT) & 0x1ff
    }

    pub fn size_class(&self) -> u32 {
        (self.flags.inner().load(Ordering::SeqCst) >> Self::SIZE_CLASS_SHIFT) & 0x3f
    }

    pub fn sv_offset(&self) -> u32 {
        (self.flags.inner().load(Ordering::SeqCst) >> Self::SHUFFLE_VECTOR_OFFSET_SHIFT) & 0xff
    }

    pub fn free_list_id(&self) -> FreeListId {
        let id = (self.flags.inner().load(Ordering::SeqCst) >> Self::FREELIST_ID_SHIFT) & 0x3;

        match id {
            0 => FreeListId::Full,
            1 => FreeListId::Partial,
            2 => FreeListId::Empty,
            3 => FreeListId::Attached,
            4 => FreeListId::Max,
            _ => unreachable!(),
        }
    }

    pub fn set_meshed(&self) {
        self.set_at(Self::MESHED_OFFSET);
    }

    pub fn unset_meshed(&self) {
        self.unset_at(Self::MESHED_OFFSET);
    }

    pub fn is_meshed(&self) -> bool {
        (self.flags.inner().load(Ordering::SeqCst) >> Self::MESHED_OFFSET) & 1 == 1
    }

    pub fn set_freelist_id(&self, freelist_id: FreeListId) {
        self.set_at(Self::MESHED_OFFSET);
        let mask = 0x3 << Self::FREELIST_ID_SHIFT;
        let new_val = (freelist_id as u32) << Self::FREELIST_ID_SHIFT;
        self.set_masked(!mask, new_val);
    }

    pub fn set_sv_offset(&self, off: u8) {
        self.set_at(Self::MESHED_OFFSET);
        let mask = 0xff << Self::SHUFFLE_VECTOR_OFFSET_SHIFT;
        let new_val = (off as u32) << Self::SHUFFLE_VECTOR_OFFSET_SHIFT;
        self.set_masked(!mask, new_val);
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreeListId {
    Full = 0,
    Partial = 1,
    Empty = 2,
    Attached = 3,
    Max = 4,
}

impl FreeListId {
    pub fn from_integer(id: u32) -> Self {
        match id {
            0 => Self::Full,
            1 => Self::Partial,
            2 => Self::Empty,
            3 => Self::Attached,
            4 => Self::Max,
            _ => unreachable!(),
        }
    }
}
const fn class_index(size: usize) -> usize {
    if size <= MAX_SMALL_SIZE {
        (size + 7) >> 3
    } else {
        (size + 127 + (120 << 7)) >> 7
    }
}
pub fn size_class(size: usize) -> u32 {
    CLASS_ARRAY[class_index(size)]
}
