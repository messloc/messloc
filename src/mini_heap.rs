use std::{
    cell::Ref,
    cell::RefCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ops::DerefMut,
    ptr::{addr_of_mut, copy_nonoverlapping, null, null_mut},
    rc::Rc,
    sync::{
        atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use lazy_static::__Deref;
use libc::c_void;

use crate::{
    bitmap::{AtomicBitmapBase, Bitmap, BitmapBase, RelaxedBitmapBase},
    class_array::CLASS_ARRAY,
    comparatomic::Comparatomic,
    list_entry::{ListEntry, Listable},
    meshable_arena::{Page, PageType},
    one_way_mmap_heap::Heap,
    runtime::Messloc,
    span::{self, Span},
    utils::builtin_prefetch,
    MAX_MESHES, MAX_SMALL_SIZE, PAGE_SIZE,
};

pub struct MiniHeap {
    runtime: Messloc,
    pub arena_begin: *mut Page,
    object_size: usize,
    pub span_start: *mut Self,
    pub free_list: ListEntry,
    bitmap: Rc<RefCell<Bitmap<AtomicBitmapBase<4>>>>,
    span: Span,
    //   internal::Bitmap _bitmap;           // 32 bytes 32
    //   const Span _span;                   // 8        40
    //   MiniHeapListEntry _free;list{};      // 8        48
    //   atomic<pid_t> _current{0};          // 4        52
    //   Flags _flags;                       // 4        56
    flags: Flags,
    //   MiniHeapID _nextMeshed{};           // 4        64
    current: Comparatomic<AtomicU64>,
    pub next_mashed: MiniHeapId,
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
        addr_of_mut!((*this).bitmap).write(Rc::new(RefCell::new(
            Bitmap::<AtomicBitmapBase<4>>::default(),
        )));
        addr_of_mut!((*this).flags).write(Flags::new(
            object_count as u32,
            if object_count > 1 {
                size_class(object_size)
            } else {
                1
            },
            0,
            FreeListId::Attached as u32,
        ));
    }

    pub fn with_object(span: Span, object_count: usize, object_size: usize) -> MiniHeap {
        todo!()
    }

    #[allow(clippy::unused_self)]
    pub fn get_mini_heap(&self, id: &MiniHeapId) -> *mut MiniHeap {
        if let MiniHeapId::HeapPointer(mh) = id {
            builtin_prefetch(id as *const _ as *mut ());
            *mh
        } else {
            unreachable!()
        }
    }

    pub fn max_count(&self) -> u32 {
        self.flags.max_count()
    }

    pub fn span_size(&self) -> usize {
        self.span.byte_length()
    }

    pub fn is_large_alloc(&self) -> bool {
        self.max_count() == 1
    }

    pub fn is_full(&self) -> bool {
        self.in_use_count() <= self.max_count() as usize
    }

    pub fn bitmap(&self) -> Rc<RefCell<Bitmap<AtomicBitmapBase<4>>>> {
        self.bitmap.clone()
    }

    pub unsafe fn malloc_at(&self, offset: usize) -> *mut () {
        let arena = self.arena_begin;

        if self.bitmap().borrow_mut().try_to_set(offset) {
            let object_size = if self.is_large_alloc() {
                self.span.length * PAGE_SIZE
            } else {
                (self.object_size as f32 + 0.5).trunc() as usize
            };
            arena
                .add(self.span.offset)
                .cast::<u8>()
                .add(offset * object_size)
                .cast()
        } else {
            null_mut()
        }
    }

    pub fn consume(&mut self, src: &MiniHeap) {
        // TODO: consider taking an owned miniheap
        assert!(src != self);
        assert_eq!(self.object_size, src.object_size);

        src.set_meshed();
        let src_span = self.span_start;
        src.take_bitmap().iter_mut().for_each(|off| {
            assert!(!self
                .bitmap
                .borrow_mut()
                .is_set(off.load(Ordering::AcqRel) as usize));

            let offset = off.load(Ordering::AcqRel) as usize;
            unsafe {
                let src_object = unsafe { src_span.add(offset) };
                let dst_object = self.malloc_at(offset) as *mut MiniHeap;
                copy_nonoverlapping(src_object, dst_object, self.object_size);
            }
        });
        self.track_meshed_span(src);
    }

    pub fn track_meshed_span(&mut self, src: &MiniHeap) {
        match self.next_mashed {
            MiniHeapId::Head => {
                self.next_mashed = MiniHeapId::HeapPointer(&src as *const _ as *mut MiniHeap);
            }
            MiniHeapId::HeapPointer(mh) => {
                let heap = unsafe { mh.as_mut().unwrap() };
                heap.track_meshed_span(src);
            }
            MiniHeapId::None => {}
        }
    }

    pub fn in_use_count(&self) -> usize {
        self.bitmap.borrow().inner().in_use_count() as usize
    }

    pub fn clear_if_not_free(&mut self, offset: usize) -> bool {
        !self.bitmap.borrow_mut().unset(offset)
    }

    pub fn set_meshed(&self) {
        self.flags.set(Flags::MESHED_OFFSET);
    }

    pub fn is_attached(&self) -> bool {
        self.current() != 0
    }

    pub fn set_attached(&mut self, current: u64, free_list: *mut ListEntry) {
        self.current.store(current, Ordering::Release);
        self.free_list.remove(free_list);
        self.set_free_list_id(FreeListId::Attached);
    }

    pub fn is_meshed(&self) -> bool {
        self.flags.is_meshed()
    }

    pub fn current(&self) -> u64 {
        self.current.load(Ordering::Acquire)
    }

    pub fn unset_attached(&self) {
        self.current.store(0, Ordering::Release);
    }

    pub fn free_list_id(&self) -> FreeListId {
        self.flags.free_list_id()
    }

    pub fn size_class(&self) -> u32 {
        self.flags.size_class()
    }

    pub fn bytes_free(&self) -> usize {
        self.in_use_count() * self.object_size
    }

    pub fn is_meshing_candidate(&self) -> bool {
        self.is_attached() && self.object_size < PAGE_SIZE
    }

    pub fn fullness(&self) -> f64 {
        self.in_use_count() as f64 / self.max_count() as f64
    }

    pub fn take_bitmap(&self) -> [Comparatomic<AtomicU64>; 4] {
        self.bitmap.borrow().set_and_exchange_all()
    }

    pub fn set_sv_offset(&mut self, i: usize) {
        //TODO: check if u8 is enough for this
        self.flags.set_sv_offset(u8::try_from(i).unwrap());
    }

    pub fn free_offset(&mut self, offset: usize) {
        self.bitmap.borrow_mut().unset(offset);
    }

    pub fn mesh_count(&self) -> usize {
        let mut count = 0;

        let mut mh = self;
        loop {
            count += 1;
            unsafe {
                if let MiniHeapId::HeapPointer(next) = mh.next_mashed {
                    builtin_prefetch(next as *const _ as *mut ());
                } else {
                    break;
                }
            }
        }

        count
    }

    pub fn set_free_list(&mut self, free_list: ListEntry) {
        self.free_list = free_list;
    }

    pub fn get_free_list_id(&self) -> FreeListId {
        self.flags.free_list_id()
    }

    pub fn set_free_list_id(&self, free_list: FreeListId) {
        self.flags.set_freelist_id(free_list)
    }
}
impl Heap for MiniHeap {
    type PointerType = *mut ();
    type MallocType = *mut ();

    unsafe fn map(&mut self, size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut () {
        todo!()
    }

    unsafe fn malloc(&mut self, size: usize) -> *mut () {
        todo!()
    }

    unsafe fn get_size(&mut self, ptr: *mut ()) -> usize {
        todo!()
    }

    unsafe fn free(&mut self, ptr: *mut ()) {
        todo!()
    }
}

impl PartialEq for MiniHeap {
    fn eq(&self, other: &Self) -> bool {
        self.object_size == other.object_size
            && self.span_start == other.span_start
            && self.bitmap == other.bitmap
            && self.span == other.span
            && self.flags == other.flags
            && self.current == other.current
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
            0 => FreeListId::Full,
            1 => FreeListId::Partial,
            2 => FreeListId::Empty,
            3 => FreeListId::Attached,
            4 => FreeListId::Max,
            _ => unreachable!(),
        }
    }
}
fn class_index(size: usize) -> usize {
    if size <= MAX_SMALL_SIZE {
        (size + 7) >> 3
    } else {
        (size + 127 + (120 << 7)) >> 7
    }
}
pub fn size_class(size: usize) -> u32 {
    CLASS_ARRAY[class_index(size)]
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

    fn set(&self, offset: u32) {
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

#[derive(Debug, Default)]
#[allow(clippy::module_name_repetitions)]
pub enum MiniHeapId {
    Head,
    HeapPointer(*mut MiniHeap),
    #[default]
    None,
}

impl MiniHeapId {
    pub fn new(ptr: *mut MiniHeap) -> Self {
        MiniHeapId::HeapPointer(ptr)
    }

    pub unsafe fn get(&self, index: usize) -> *mut () {
        match self {
            MiniHeapId::HeapPointer(mh) => mh.add(index) as *mut (),
            _ => unreachable!(),
        }
    }

    pub fn is_head(&self) -> bool {
        matches!(self, MiniHeapId::Head)
    }
}

#[macro_export]
macro_rules! for_each_meshed {
    ($mh: tt $func: block) => {{
        let result = loop {
            if let MiniHeapId::HeapPointer(value) = $mh.next_mashed {
                let mut result = false;
                result = $func;
                if result && let p @ &MiniHeapId::HeapPointer(val) = &$mh.next_mashed {
                                                                   let mh = $mh.get_mini_heap(&p);
                                                                    } else {
                                                                        break true;
                                                                    }
            } else {
                todo!()
            }
        };
        result
    }};
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
