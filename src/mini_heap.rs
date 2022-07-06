use std::{sync::atomic::{AtomicU32, Ordering}, num::NonZeroU32};

use crate::{
    bitmap::{AtomicBitmap, Bitmap},
    meshable_arena::Span, consts::PAGE_SIZE, list,
};

pub type MiniHeapId = Option<NonZeroU32>;

#[derive(Default)]
pub struct MiniHeapListEntry {
    pub prev: MiniHeapId,
    pub next: MiniHeapId,
}

pub struct MiniHeap {
    bitmap: AtomicBitmap,
    pub span: Span,
    free_list: MiniHeapListEntry,
    current: AtomicU32,
    flags: Flags,
    object_size_reciprocal: f32,
    next_meshed: MiniHeapId,
}
impl MiniHeap {
    pub fn new(arena_begin: *const (), span: Span, object_count: usize, object_size: usize) {
        let bitmap = AtomicBitmap::new(object_count);
        let flags = Flags::new(object_count as u32, todo!(), 0, todo!());

        debug_assert_eq!(bitmap.in_use_count(), 0);
        let expected_span_size = span.byte_length();
    }
    #[inline(always)]
    pub fn free(&mut self, arena_begin: *const (), ptr: *const ()) {
        // the logic in globalFree is
        // updated to allow the 'race' between lock-free freeing and
        // meshing
        // d_assert(!isMeshed());
        let off = self.get_off(arena_begin, ptr);
        if off < 0 {
            return;
        }

        self.free_off(off);
    }

    pub fn get_off(&self, arena_begin: *const (), ptr: *const ()) -> u8 {
        let span = self.span_start(arena_begin as usize, ptr);
        debug_assert_ne!(span, 0);
        let ptrval = ptr as usize;

        let off = ((ptrval - span) as f32 * self.object_size_reciprocal) as u8;
        debug_assert!(off < self.max_count());
        off
    }
    fn span_start(&self, arena_begin: usize, ptr: *const ()) -> usize {
        let ptrval = ptr as usize;
        let len = self.span.byte_length();


        // manually unroll loop once to capture the common case of
        // un-meshed miniheaps
        let spanptr = arena_begin + self.span.offset * PAGE_SIZE;
        if /* likely */ (spanptr..spanptr + len).contains(&ptrval) {
            spanptr
        } else {
            // slow path
            let spanptr = 0;

            let mut mh = self;
            loop {
                if /* unlikely */ mh.next_meshed.is_none() {
                    std::process::abort();
                }
                mh = 
            }

        }
    }
}

pub struct Flags(AtomicU32);

impl Flags {
    const SIZE_CLASS_SHIFT: u32 = 0;
    const FREE_LIST_ID_SHIFT: u32 = 6;
    const SHUFFLE_VECTOR_OFFSET_SHIFT: u32 = 8;
    const MAX_COUNT_SHIFT: u32 = 16;
    const MESHED_OFFSET: usize = 30;

    pub fn new(
        max_count: u32,
        size_class: u32,
        shuffle_vector_offset: u32,
        free_list_id: u32,
    ) -> Self {
        #[rustfmt::skip]
        let val = max_count << Self::MAX_COUNT_SHIFT
            + size_class << Self::SIZE_CLASS_SHIFT
            + shuffle_vector_offset << Self::SHUFFLE_VECTOR_OFFSET_SHIFT
            + free_list_id << Self::FREE_LIST_ID_SHIFT;

        let mut this = Self(AtomicU32::new(val));

        debug_assert_eq!(free_list_id & 0x3, free_list_id);
        debug_assert_eq!(
            size_class & ((1 << Self::FREE_LIST_ID_SHIFT) - 1),
            size_class
        );
        debug_assert!(shuffle_vector_offset < 255);
        debug_assert!(size_class < 255, "size_class: {size_class}");
        debug_assert!(max_count <= 256);
        debug_assert_eq!(this.max_count(), max_count);

        this
    }
    pub fn free_list_id(&self) -> u32 {
        (self.0.load(Ordering::SeqCst) >> Self::FREE_LIST_ID_SHIFT) & 0x3
    }
    pub fn set_free_list_id(&self, id: u32) {
        //static_assert(list::Max <= (1 << FreelistIdShift), "expected max < 4");
        //d_assert(freelistId < list::Max);
        let mask = !(0x3u32 << Self::FREE_LIST_ID_SHIFT);
        let new = id << Self::FREE_LIST_ID_SHIFT;
        self.set_masked(mask, new)
    }
    pub fn max_count(&self) -> u32 {
        (self.0.load(Ordering::SeqCst) >> Self::MAX_COUNT_SHIFT) & 0x1ff
    }
    pub fn size_class(&self) -> u32 {
        (self.0.load(Ordering::SeqCst) >> Self::SIZE_CLASS_SHIFT) & 0x3f
    }
    pub fn shuffle_vector_offset(&self) -> u32 {
        (self.0.load(Ordering::SeqCst) >> Self::SHUFFLE_VECTOR_OFFSET_SHIFT) & 0xff
    }
    pub fn set_shuffle_vector_offset(&self, offset: u32) {
        debug_assert!(offset < 255);
        let mask = !(0xffu32 << Self::SHUFFLE_VECTOR_OFFSET_SHIFT);
        let new = offset << Self::SHUFFLE_VECTOR_OFFSET_SHIFT;
        self.set_masked(mask, new)
    }
    pub fn is_meshed(&self) -> bool {
        self.is(Self::MESHED_OFFSET)
    }
    pub fn set_meshed(&self) {
        self.set(Self::MESHED_OFFSET)
    }
    pub fn unset_meshed(&self) {
        self.unset(Self::MESHED_OFFSET)
    }

    fn is(&self, offset: usize) -> bool {
        let mask = 1 << offset;
        self.0.load(Ordering::Acquire) & mask == mask
    }
    fn set(&self, offset: usize) {
        let mask = 1 << offset;
        let old = self.0.load(Ordering::Relaxed);

        while self
            .0
            .compare_exchange_weak(old, old | mask, Ordering::Release, Ordering::Relaxed)
            .is_err()
        {}
    }
    fn unset(&self, offset: usize) {
        let mask = 1 << offset;
        let old = self.0.load(Ordering::Relaxed);

        while self
            .0
            .compare_exchange_weak(old, old & !mask, Ordering::Release, Ordering::Relaxed)
            .is_err()
        {}
    }
    fn set_masked(&self, mask: u32, new: u32) {
        let old = self.0.load(Ordering::Relaxed);

        while self
            .0
            .compare_exchange_weak(old, old & mask | new, Ordering::Release, Ordering::Relaxed)
            .is_err()
        {}
    }
}

fn get_mini_heap(id: MiniHeapId) -> *mut MiniHeap {
    assert!(id.is_some() && id != list::HEAD);
    runtime().heap().miniheap_for_id(id)
}
fn get_mini_heap_id(mh: *mut MiniHeap) -> MiniHeapId {
    if /* unlikely */ mh.is_null() {
        debug_assert!(false);
        None
    } else {
        runtime().heap().miniheap_id_for(id)
    }
}

