use crate::comparatomic::Comparatomic;
use crate::mini_heap::{MiniHeap, MiniHeapId};
use crate::utils::mmap;
use crate::{one_way_mmap_heap, ARENA_SIZE};
use crate::{
    one_way_mmap_heap::{Heap, OneWayMmapHeap},
    PAGE_SIZE,
};

use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::sync::atomic::Ordering;
use core::{mem::size_of, ptr::null_mut};

pub struct CheapHeap<const ALLOC_SIZE: usize, const MAX_COUNT: usize> {
    freelist: *mut (), // [*mut [u8; ALLOC_SIZE]; MAX_COUNT]
    arena_offset: usize,
    freelist_offset: usize,
}

impl<const ALLOC_SIZE: usize, const MAX_COUNT: usize> CheapHeap<ALLOC_SIZE, MAX_COUNT> {
    pub fn new() -> Self {
        let mut this = Self {
            freelist: null_mut(),
            arena_offset: 0,
            freelist_offset: 0,
        };

        this.freelist = unsafe {
            OneWayMmapHeap
                .malloc(MAX_COUNT * size_of::<*mut [u8; ALLOC_SIZE]>())
                .cast()
        };
        this
    }

    pub unsafe fn alloc(&mut self) -> *mut () {
        if self.freelist_offset > 0 {
            self.freelist_offset -= 1;
            return self
                .freelist
                .cast::<*mut ()>()
                .add(self.freelist_offset)
                .read();
        }

        self.arena_offset += 1;
        OneWayMmapHeap.malloc(ALLOC_SIZE)
    }

    pub unsafe fn assign(&mut self, data: *mut (), size: usize) {
        let ptr = self as *mut Self as *mut ();
        ptr.copy_from_nonoverlapping(data, size);
    }
}

impl<const ALLOC_SIZE: usize, const MAX_COUNT: usize> Heap for CheapHeap<ALLOC_SIZE, MAX_COUNT> {
    type MallocType = [MiniHeapId; ARENA_SIZE / PAGE_SIZE];
    type PointerType = MiniHeapId;
    unsafe fn map(
        &mut self,
        size: usize,
        flags: libc::c_int,
        fd: libc::c_int,
    ) -> Self::PointerType {
        let size = (size + PAGE_SIZE - 1) & (PAGE_SIZE - 1);
        let ptr = mmap(null_mut(), fd, size, 0).unwrap();
        MiniHeapId::HeapPointer(ptr as *mut MiniHeap)
    }

    unsafe fn malloc(&mut self, size: usize) -> *mut Self::MallocType {
        let addr = OneWayMmapHeap.malloc(size) as *mut Self;
        let page_data = OneWayMmapHeap.malloc(ARENA_SIZE / PAGE_SIZE) as *mut Self::PointerType;

        (0..ARENA_SIZE)
            .step_by(PAGE_SIZE)
            .enumerate()
            .for_each(|(index, page)| {
                let heap_addr = addr.add(page);
                let page_addr = page_data.add(index);
                let heap =
                    OneWayMmapHeap.malloc(core::mem::size_of::<MiniHeapId>()) as *mut MiniHeapId;
                heap.write(MiniHeapId::HeapPointer(null_mut()));
                page_addr.copy_from(heap, 1);
            });

        page_data as *mut Self::MallocType
    }

    unsafe fn grow<T>(&mut self, src: *mut T, old: usize, new: usize) -> *mut T { 
        todo!()
    }

    unsafe fn get_size(&mut self, _: *mut ()) -> usize {
        ALLOC_SIZE
    }

    unsafe fn free(&mut self, ptr: *mut ()) {
        debug_assert_eq!(
            ptr.align_offset(core::mem::align_of::<[u8; ALLOC_SIZE]>()),
            0,
            "ptr must be aligned to our alloc size"
        );
        let ptr = ptr.cast::<[u8; ALLOC_SIZE]>();

        //   self.freelist.add(self.freelist_offset).write(ptr as *mut ());

        self.freelist_offset += 1;
    }
}

impl<const AC: usize, const MC: usize> Default for CheapHeap<AC, MC> {
    fn default() -> Self {
        Self::new()
    }
}
unsafe impl<const ALLOC_SIZE: usize, const MAX_COUNT: usize> Sync
    for CheapHeap<ALLOC_SIZE, MAX_COUNT>
{
}

pub struct DynCheapHeap {
    arena: *mut u8,
    freelist: *mut *mut u8,
    arena_offset: usize,
    freelist_offset: usize,
    alloc_size: usize,
    max_count: usize,
}

impl DynCheapHeap {
    pub fn new(
        alloc_size: usize,
        max_count: usize,
        arena: *mut u8,
        freelist: *mut *mut u8,
    ) -> Self {
        DynCheapHeap {
            arena,
            freelist,
            arena_offset: 0,
            freelist_offset: 0,
            alloc_size,
            max_count,
        }
    }

    pub unsafe fn alloc(&mut self) -> *mut u8 {
        if self.freelist_offset > 0 {
            self.freelist_offset -= 1;
            return self.freelist.add(self.freelist_offset).read();
        }

        self.arena_offset += 1;
        self.arena.add(self.arena_offset * self.alloc_size)
    }

    const fn arena_end(&self) -> *mut u8 {
        unsafe { self.arena.add(self.alloc_size * self.max_count) }
    }
}

impl Heap for DynCheapHeap {
    type PointerType = *mut ();
    type MallocType = ();

    unsafe fn map(&mut self, size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut () {
        todo!()
    }

    unsafe fn malloc(&mut self, size: usize) -> *mut () {
        OneWayMmapHeap.malloc(size)
    }

    unsafe fn grow<T>(&mut self, src: *mut T, old: usize, new: usize) -> *mut T { 
    todo!()
    }

    unsafe fn get_size(&mut self, _: *mut ()) -> usize {
        self.alloc_size

    }

    unsafe fn free(&mut self, ptr: *mut ()) {
        debug_assert_eq!(
            ptr.align_offset(self.alloc_size),
            0,
            "ptr must be aligned to our alloc size"
        );
        let ptr = ptr.cast::<u8>();

        debug_assert!(
            (self.arena..self.arena_end()).contains(&ptr),
            "ptr must reside in our arena"
        );

        self.freelist.add(self.freelist_offset).write(ptr);
        self.freelist_offset += 1;
    }
}
