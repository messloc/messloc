use std::{
    ptr::{addr_of_mut, null_mut},
    sync::atomic::{AtomicPtr, AtomicU32, Ordering},
};

use libc::c_void;

use crate::{
    atomic_bitmap::AtomicBitmap256, class_array::CLASS_ARRAY, comparatomic::Comparatomic,
    one_way_mmap_heap::Heap, span::Span, MAX_SMALL_SIZE, PAGE_SIZE,
};

#[derive(PartialEq, Default)]
pub struct MiniHeap {
    bitmap: AtomicBitmap256,
    span: Span,
    //   internal::Bitmap _bitmap;           // 32 bytes 32
    //   const Span _span;                   // 8        40
    //   MiniHeapListEntry _freelist{};      // 8        48
    //   atomic<pid_t> _current{0};          // 4        52
    //   Flags _flags;                       // 4        56
    flags: Flags,
    object_size_reciprocal: f32, // 4        60
                                 //   MiniHeapID _nextMeshed{};           // 4        64
}

#[repr(u32)]
pub enum FreelistId {
    Full = 0,
    Partial = 1,
    Empty = 2,
    Attached = 3,
    Max = 4,
}

fn class_index(size: usize) -> usize {
    if size <= MAX_SMALL_SIZE {
        (size + 7) >> 3
    } else {
        (size + 127 + (120 << 7)) >> 7
    }
}
fn size_class(size: usize) -> u32 {
    CLASS_ARRAY[class_index(size)]
}

impl MiniHeap {
    // creates the MiniHeap at the location of the pointer
    pub unsafe fn new_inplace(
        this: *mut Self,
        span: Span,
        object_count: usize,
        object_size: usize,
    ) {
        addr_of_mut!((*this).span).write(span);
        addr_of_mut!((*this).object_size_reciprocal).write((object_size as f32).recip());
        addr_of_mut!((*this).bitmap).write(AtomicBitmap256::default());
        addr_of_mut!((*this).flags).write(Flags::new(
            object_count as u32,
            if object_count > 1 {
                size_class(object_size)
            } else {
                1
            },
            0,
            FreelistId::Attached as u32,
        ));
    }

    pub fn max_count(&self) -> u32 {
        todo!()
    }

    pub fn span_size(&self) -> usize {
        self.span.byte_length()
    }

    pub fn is_large_alloc(&self) -> bool {
        self.max_count() == 1
    }

    pub unsafe fn malloc_at(&self, arena: *mut [u8; PAGE_SIZE], offset: usize) -> *mut () {
        if !self.bitmap.try_to_set(offset as u64) {
            null_mut()
        } else {
            let object_size = if self.is_large_alloc() {
                self.span.length as usize * PAGE_SIZE
            } else {
                (self.object_size_reciprocal.recip() + 0.5) as usize
            };
            arena
                .add(self.span.offset as usize)
                .cast::<u8>()
                .add(offset * object_size)
                .cast()
        }
    }

    pub unsafe fn get_span_start(&self, addr: *mut crate::meshable_arena::Page) -> *mut c_void {
        addr.add(self.span.length as usize * PAGE_SIZE) as *mut c_void
    }
}

#[derive(PartialEq, Default)]
pub struct Flags {
    flags: Comparatomic<AtomicU32>,
}

impl Flags {
    const SIZE_CLASS_SHIFT: u32 = 0;
    const FREELIST_ID_SHIFT: u32 = 6;
    const SHUFFLE_VECTOR_OFFSET_SHIFT: u32 = 8;
    const MAX_COUNT_SHIFT: u32 = 16;
    const MESHED_OFFSET: u32 = 30;

    pub fn new(max_count: u32, size_class: u32, sv_offset: u32, freelist_id: u32) -> Self {
        let flags = (max_count << Self::MAX_COUNT_SHIFT)
            + (size_class << Self::SIZE_CLASS_SHIFT)
            + (sv_offset << Self::SHUFFLE_VECTOR_OFFSET_SHIFT)
            + (freelist_id << Self::FREELIST_ID_SHIFT);
        Self {
            flags: Comparatomic::new(flags),
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

    pub fn freelist_id(&self) -> u32 {
        (self.flags.inner().load(Ordering::SeqCst) >> Self::FREELIST_ID_SHIFT) & 0x3
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

    pub fn set_freelist_id(&self, id: u32) {
        self.set_at(Self::MESHED_OFFSET);
        let mask = 0x3 << Self::FREELIST_ID_SHIFT;
        let new_val = id << Self::FREELIST_ID_SHIFT;
        self.set_masked(!mask, new_val);
    }

    pub fn set_sv_offset(&self, off: u8) {
        self.set_at(Self::MESHED_OFFSET);
        let mask = 0xff << Self::SHUFFLE_VECTOR_OFFSET_SHIFT;
        let new_val = (off as u32) << Self::SHUFFLE_VECTOR_OFFSET_SHIFT;
        self.set_masked(!mask, new_val);
    }
}

#[derive(Clone, Debug, Copy)]
pub struct MiniHeapId(pub u32);

impl MiniHeapId {}

// FIXME:: replace this with MiniHeapId and make it atomic if all usages of MiniHeapId are atomic
// FIXME:: consider whether we need to multiply the array size by size of usize
#[derive(Debug)]

pub struct AtomicMiniHeapId<T: Heap>(AtomicPtr<T>);

impl<T: Heap> AtomicMiniHeapId<T> {
    pub fn new(ptr: *mut T) -> AtomicMiniHeapId<T> {
        AtomicMiniHeapId(AtomicPtr::new(ptr))
    }

    pub fn inner(&mut self) -> *mut T {
        self.0.load(Ordering::Acquire)
    }

    pub unsafe fn get(&mut self, index: usize) -> *mut T {
        let ptr = self.0.get_mut();
        ptr.add(index)
    }

    pub fn load(&self, ordering: Ordering) -> *mut T {
        self.0.load(ordering)
    }

    pub fn store(&self, value: *mut T, ordering: Ordering) {
        self.0.store(value, ordering)
    }
}

impl<T: Heap> Default for AtomicMiniHeapId<T> {
    fn default() -> Self {
        todo!()
    }
}
// class Flags {
//     private:
//       DISALLOW_COPY_AND_ASSIGN(Flags);

//       static constexpr uint32_t SizeClassShift = 0;
//       static constexpr uint32_t FreelistIdShift = 6;
//       static constexpr uint32_t ShuffleVectorOffsetShift = 8;
//       static constexpr uint32_t MaxCountShift = 16;
//       static constexpr uint32_t MeshedOffset = 30;

//     public:
//       explicit Flags(uint32_t maxCount, uint32_t sizeClass, uint32_t svOffset, uint32_t freelistId) noexcept
//           : _flags{(maxCount << MaxCountShift) + (sizeClass << SizeClassShift) + (svOffset << ShuffleVectorOffsetShift) +
//                    (freelistId << FreelistIdShift)} {
//         d_assert((freelistId & 0x3) == freelistId);
//         d_assert((sizeClass & ((1 << FreelistIdShift) - 1)) == sizeClass);
//         d_assert(svOffset < 255);
//         d_assert_msg(sizeClass < 255, "sizeClass: %u", sizeClass);
//         d_assert(maxCount <= 256);
//         d_assert(this->maxCount() == maxCount);
//       }

//     };
