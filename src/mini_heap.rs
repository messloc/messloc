use std::{
    ptr::{addr_of_mut, copy_nonoverlapping, null, null_mut},
    sync::{
        atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use libc::c_void;

use crate::{
    bitmap::{AtomicBitmapBase, Bitmap, BitmapBase, RelaxedBitmapBase},
    class_array::CLASS_ARRAY,
    comparatomic::Comparatomic,
    list_entry::{ListEntry, Listable},
    one_way_mmap_heap::Heap,
    runtime::Runtime,
    span::Span,
    MAX_SMALL_SIZE, PAGE_SIZE,
};

#[derive(PartialEq)]
pub struct MiniHeap<'a> {
    id: u64,
    runtime: Runtime<'a>,
    object_size: usize,
    free_list: ListEntry<'a>,
    bitmap: Bitmap<AtomicBitmapBase<4>>,
    span: Span,
    //   internal::Bitmap _bitmap;           // 32 bytes 32
    //   const Span _span;                   // 8        40
    //   MiniHeapListEntry _free;list{};      // 8        48
    //   atomic<pid_t> _current{0};          // 4        52
    //   Flags _flags;                       // 4        56
    flags: Flags,
    object_size_reciprocal: f32, // 4        60
    //   MiniHeapID _nextMeshed{};           // 4        64
    current: Comparatomic<AtomicU64>,
    next_mashed: Option<AtomicMiniHeapId<MiniHeap<'a>>>,
}

impl<'a> MiniHeap<'a> {
    // creates the MiniHeap at the location of the pointer
    pub unsafe fn new_inplace(
        this: *mut Self,
        span: Span,
        object_count: usize,
        object_size: usize,
    ) {
        addr_of_mut!((*this).span).write(span);
        addr_of_mut!((*this).object_size_reciprocal).write((object_size as f32).recip());
        addr_of_mut!((*this).bitmap).write(Bitmap::<AtomicBitmapBase<4>>::default());
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

    pub fn with_object(span: Span, object_count: usize, object_size: usize) -> MiniHeap<'a> {
        todo!()
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn get_mini_heap(&self, id: AtomicMiniHeapId<MiniHeap<'_>>) -> *mut MiniHeap<'_> {
        self.runtime.global_heap().mini_heap_for_id(id)
    }

    pub fn get_mini_heap_id(&self, miniheap: *mut ()) -> *mut MiniHeap<'a> {
        self.runtime.global_heap().mini_heap_for(miniheap)
    }

    pub const fn max_count(&self) -> u32 {
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

    pub fn bitmap(&self) -> &Bitmap<AtomicBitmapBase<4>> {
        &self.bitmap
    }

    pub unsafe fn malloc_at(&self, arena: *mut [u8; PAGE_SIZE], offset: usize) -> *mut () {
        if !self.bitmap.try_to_set(offset) {
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

    pub fn consume(&self, mut src: &MiniHeap<'a>) {
        // TODO: consider taking an owned miniheap
        assert!(src != self);
        assert_eq!(self.object_size, src.object_size);

        src.set_meshed();
        let begin = ((*self.runtime.lock().unwrap())
            .global_heap
            .guarded
            .lock()
            .unwrap())
        .arena
        .arena_begin;
        let src_span = unsafe { src.get_span_start()};
        src.take_bitmap().iter().for_each(|off| {
            assert!(!self.bitmap.is_set(off));

            let offset = off.load(Ordering::AcqRel) as usize;
            unsafe {
                let src_object = unsafe { src_span.add(offset) };
                let dst_object = self.malloc_at(begin.cast(), offset);
                copy_nonoverlapping(src_object, dst_object as *mut c_void, self.object_size);
            }
        });
        self.track_meshed_span(self.get_mini_heap_id(src as *const MiniHeap<'_> as *mut ()));
    }

    pub fn track_meshed_span(&self, src: *mut MiniHeap<'_>) {
        if let Some(mesh) = self.next_mashed {
            unsafe {
                self.get_mini_heap(mesh)
                    .as_ref()
                    .unwrap()
                    .track_meshed_span(src);
            }
        } else {
            self.next_mashed = Some(AtomicMiniHeapId::new(src));
        }
    }

    pub unsafe fn get_span_start(&self) -> *mut c_void {
        let addr = (*self.runtime.lock().unwrap()).global_heap.guarded.lock().unwrap().arena.arena_begin;
        addr.add(self.span.length as usize * PAGE_SIZE) as *mut c_void
    }

    pub fn in_use_count(&self) -> usize {
        self.bitmap.inner().in_use_count() as usize
    }

    pub fn clear_if_not_free(&self, offset: usize) -> bool {
        !self.bitmap.inner().unset(offset)
    }

    pub fn set_meshed(&self) {
        self.flags.set(Flags::MESHED_OFFSET);
    }

    pub fn is_attached(&self) -> bool {
        self.current() != 0
    }

    pub fn set_attached(&self, current: u64, free_list: &ListEntry<'a>) {
        self.current.store(current, Ordering::Release);
        self.free_list.remove(*free_list);
        self.set_free_list_id(FreeListId::Attached);
    }

    pub fn is_meshed(&self) -> bool {
        self.flags.is_meshed()
    }

    pub fn get_free_list(&self) -> &ListEntry<'_> {
        &self.free_list
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

    pub fn is_related(&self, other: &MiniHeap<'_>) -> bool {
        self.meshed_contains(|mh| mh == other)
    }
    pub fn take_bitmap(&mut self) -> [Comparatomic<AtomicU64>; 4] {
        self.bitmap.set_and_exchange_all()
    }

    pub fn bitmap_mut(&mut self) -> &mut Bitmap<AtomicBitmapBase<4>> {
        &mut self.bitmap
    }

    pub fn set_sv_offset(&mut self, i: usize) {
        //TODO: check if u8 is enough for this
        self.flags.set_sv_offset(u8::try_from(i).unwrap());
    }

    pub fn free_offset(&mut self, offset: usize) {
        self.bitmap.unset(offset);
    }

    pub fn mesh_count(&self) -> usize {
        let mut count = 0;

        let mh = self;
        loop {
            count += 1;
            if let Some(next) = mh.next_mashed {
                mh = unsafe { self.get_mini_heap(next).as_ref().unwrap() };
            } else {
                break;
            }
        }

        count
    }

    pub fn for_each_meshed<F>(&self, func: F)
    where
        F: Fn(&MiniHeap<'_>) -> bool,
    {
        loop {
            if !func(self) && let Some(next_mashed) = self.next_mashed {
               let mh = self.get_mini_heap(next_mashed);
                unsafe {
               (*mh).for_each_meshed(func);
                }
            }
        }
    }

    pub fn meshed_contains<F>(&self, predicate: F) -> bool
    where
        F: Fn(&MiniHeap<'_>) -> bool,
    {
        loop {
            if let Some(next_mashed) = self.next_mashed {
                if !predicate(self) {
                    let mh = self.get_mini_heap(next_mashed);

                    unsafe { (*mh).for_each_meshed(predicate) };
                } else {
                    return true;
                }
            }
        }
    }

    pub fn set_free_list(&mut self, free_list: ListEntry<'_>) {
        self.free_list = free_list;
    }

    pub fn get_free_list_id(&self) -> FreeListId {
        self.flags.free_list_id()
    }

    pub fn set_free_list_id(&mut self, free_list: FreeListId) {
        self.flags.set_freelist_id(free_list)
    }

}

impl Heap for MiniHeap<'_> {
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

#[derive(Clone, Debug, Copy)]
pub struct MiniHeapId(pub u32);

impl MiniHeapId {}

// FIXME:: replace this with MiniHeapId and make it atomic if all usages of MiniHeapId are atomic
// FIXME:: consider whether we need to multiply the array size by size of usize
#[derive(PartialEq)]
pub struct AtomicMiniHeapId<T: Heap>(Comparatomic<AtomicPtr<T>>);

impl<T: Heap> AtomicMiniHeapId<T> {
    pub fn new(ptr: *mut T) -> AtomicMiniHeapId<T> {
        AtomicMiniHeapId(Comparatomic::new(ptr))
    }

    pub fn inner(&mut self) -> *mut T {
        self.0.load(Ordering::Acquire)
    }

    pub unsafe fn get(&mut self, index: usize) -> *mut T {
        let ptr = self.0.inner().get_mut();
        ptr.add(index)
    }

    pub fn load(&self, ordering: Ordering) -> *mut T {
        self.0.load(ordering)
    }

    pub fn store(&self, value: *mut T, ordering: Ordering) {
        self.0.store(value, ordering)
    }

    pub fn is_head(&self) -> bool {
        self.load(Ordering::Acquire) == null::<T>() as *mut T
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
