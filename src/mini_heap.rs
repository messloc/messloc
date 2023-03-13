use core::{
    cell::Ref,
    cell::RefCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ops::DerefMut,
    ptr::{addr_of_mut, copy_nonoverlapping, null, null_mut},
    sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering},
};

use libc::c_void;

use crate::{
    bitmap::{AtomicBitmapBase, Bitmap, BitmapBase, RelaxedBitmapBase},
    class_array::CLASS_ARRAY,
    comparatomic::Comparatomic,
    fake_std::Initer,
    flags::{size_class, Flags, FreeListId},
    list_entry::{ListEntry, Listable},
    meshable_arena::{Page, PageType},
    one_way_mmap_heap::Heap,
    span::{self, Span},
    utils::builtin_prefetch,
    MAX_MESHES, MAX_SMALL_SIZE, PAGE_SIZE,
};

pub struct MiniHeap {
    pub arena_begin: *mut Page,
    pub object_size: usize,
    pub span_start: *mut Self,
    pub free_list: ListEntry,
    pub bitmap: Bitmap<AtomicBitmapBase<4>>,
    span: Span,
    //   internal::Bitmap _bitmap;           // 32 bytes 32
    //   const Span _span;                   // 8        40
    //   MiniHeapListEntry _free;list{};      // 8        48
    //   atomic<pid_t> _current{0};          // 4        52
    //   Flags _flags;                       // 4        56
    pub flags: Flags,
    //   MiniHeapID _nextMeshed{};           // 4        64
    pub current: Comparatomic<AtomicU64>,
    pub next_mashed: MiniHeapId,
}

impl MiniHeap {
    pub unsafe fn new(start: *mut (), span: Span, object_size: usize) -> Self {
        let object_count = 0;

        let flags = Flags::new(
            object_count as u32,
            if object_count > 1 {
                size_class(PAGE_SIZE)
            } else {
                1
            },
            0,
            FreeListId::Attached as u32,
        );
        MiniHeap {
            arena_begin: start.cast(),
            object_size,
            span_start: null_mut(),
            free_list: ListEntry::default(),
            bitmap: Bitmap::default(),
            span,
            flags,
            current: Comparatomic::new(0),
            next_mashed: MiniHeapId::None,
        }
    }

    // creates the MiniHeap at the location of the pointer
    pub unsafe fn new_inplace(
        this: *mut Self,
        span: Span,
        object_count: usize,
        object_size: usize,
    ) {
        addr_of_mut!((*this).arena_begin).write(null_mut());
        addr_of_mut!((*this).span_start).write(null_mut());
        addr_of_mut!((*this).free_list).write(ListEntry::default());
        addr_of_mut!((*this).span).write(span);
        addr_of_mut!((*this).object_size).write(object_size);
        addr_of_mut!((*this).current).write(Comparatomic::new(0));

        addr_of_mut!((*this).next_mashed).write(MiniHeapId::None);
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

    pub fn with_object(span: Span, object_count: usize, object_size: usize) -> Self {
        todo!()
    }

    #[allow(clippy::unused_self)]
    pub fn get_mini_heap(&self, id: &MiniHeapId) -> *mut Self {
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

    pub unsafe fn malloc_at(&mut self, offset: usize) -> *mut () {
        let arena = self.arena_begin;

        if self.bitmap.try_to_set(offset) {
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

    pub fn consume(&mut self, src: &Self) {
        // TODO: consider taking an owned miniheap
        assert!(src != self);
        assert_eq!(self.object_size, src.object_size);

        src.set_meshed();
        let src_span = self.span_start;
        src.take_bitmap().iter_mut().for_each(|off| {
            assert!(!self.bitmap.is_set(off.load(Ordering::Acquire) as usize));

            let offset = off.load(Ordering::Acquire) as usize;
            unsafe {
                let src_object = unsafe { src_span.add(offset) };
                let dst_object = self.malloc_at(offset) as *mut MiniHeap;
                copy_nonoverlapping(src_object, dst_object, self.object_size);
            }
        });
        self.track_meshed_span(src);
    }

    pub fn track_meshed_span(&mut self, src: &Self) {
        match self.next_mashed {
            MiniHeapId::Head => {
                self.next_mashed = MiniHeapId::HeapPointer(&src as *const _ as *mut Self);
            }
            MiniHeapId::HeapPointer(mh) => {
                let heap = unsafe { mh.as_mut().unwrap() };
                heap.track_meshed_span(src);
            }
            MiniHeapId::None => {}
        }
    }

    pub fn in_use_count(&self) -> usize {
        self.bitmap.inner().in_use_count() as usize
    }

    pub fn clear_if_not_free(&mut self, offset: usize) -> bool {
        !self.bitmap.unset(offset)
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
        self.bitmap.set_and_exchange_all()
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
        self.flags.set_freelist_id(free_list);
    }
}

impl std::fmt::Debug for MiniHeap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "Miniheap debuggaa {:?}", self.arena_begin)
    }
}
impl Heap for MiniHeap {
    type PointerType = *mut ();
    type MallocType = ();

    unsafe fn map(&mut self, size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut () {
        todo!()
    }

    unsafe fn malloc(&mut self, size: usize) -> *mut () {
        todo!()
    }

    unsafe fn grow<T>(&mut self, src: *mut T, old: usize, new: usize) -> *mut T {
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

impl Initer for MiniHeap {
    fn init() -> MaybeUninit<Self> {
        MaybeUninit::uninit()
    }
}

impl Drop for MiniHeap {
    fn drop(&mut self) {
        dbg!("mini heap going down");
    }
}

#[derive(Debug, Default, PartialEq)]
#[allow(clippy::module_name_repetitions)]
pub enum MiniHeapId {
    Head,
    HeapPointer(*mut MiniHeap),
    #[default]
    None,
}

impl MiniHeapId {
    pub const fn new(ptr: *mut MiniHeap) -> Self {
        Self::HeapPointer(ptr)
    }

    pub unsafe fn get(&self, index: usize) -> *mut () {
        match self {
            Self::HeapPointer(mh) => mh.add(index) as *mut (),
            _ => unreachable!(),
        }
    }

    pub const fn is_head(&self) -> bool {
        matches!(self, Self::Head)
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
                break false;
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

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use crate::one_way_mmap_heap::{Heap, OneWayMmapHeap};
    use crate::span::Span;

    use super::MiniHeap;

    #[test]
    pub fn in_place_works() {
        unsafe {
            let h = OneWayMmapHeap.malloc(256) as *mut MiniHeap;
            MiniHeap::new_inplace(h, Span::default(), 16, 16);
            let mh = h.as_mut().unwrap();

            assert_eq!(mh.span, Span::default());
        }
    }

    #[test]
    pub fn test_dyn_array_of_mini_heaps() {
        let mut h = crate::fake_std::dynarray::DynArray::<MiniHeap, 32>::create();
        let slice = unsafe { h.as_mut_slice() };

        unsafe {
            slice.as_mut().unwrap().iter().for_each(|mh| {
                dbg!(mh.read());
            })
        };
    }
}
