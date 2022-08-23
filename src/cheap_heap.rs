use crate::mini_heap::{AtomicMiniHeapId, MiniHeap};
use crate::utils::mmap;
use crate::ARENA_SIZE;
use crate::{
    one_way_mmap_heap::{Heap, OneWayMmapHeap},
    PAGE_SIZE,
};
use std::mem::MaybeUninit;
use std::sync::atomic::Ordering;
use std::{mem::size_of, ptr::null_mut};

pub struct CheapHeap<const ALLOC_SIZE: usize, const MAX_COUNT: usize> {
    arena: *mut [u8; ALLOC_SIZE],         // [[u8; ALLOC_SIZE]; MAX_COUNT]
    freelist: *mut *mut [u8; ALLOC_SIZE], // [*mut [u8; ALLOC_SIZE]; MAX_COUNT]
    arena_offset: usize,
    freelist_offset: usize,
}

impl<const ALLOC_SIZE: usize, const MAX_COUNT: usize> CheapHeap<ALLOC_SIZE, MAX_COUNT> {
    pub fn new() -> Self {
        let mut this = CheapHeap {
            arena: null_mut(),
            freelist: null_mut(),
            arena_offset: 0,
            freelist_offset: 0,
        };
        this.arena = unsafe { OneWayMmapHeap.malloc(ALLOC_SIZE * MAX_COUNT).cast() };
        this.freelist = unsafe {
            OneWayMmapHeap
                .malloc(MAX_COUNT * size_of::<*mut [u8; ALLOC_SIZE]>())
                .cast()
        };
        this
    }

    pub unsafe fn get_mut(&self, id: AtomicMiniHeapId<MiniHeap>) -> *mut MiniHeap {
        let value = id.load(Ordering::SeqCst);

        *value.cast()
    }

    pub unsafe fn alloc(&mut self) -> *mut [u8; ALLOC_SIZE] {
        if self.freelist_offset > 0 {
            self.freelist_offset -= 1;
            return self.freelist.add(self.freelist_offset).read();
        }

        self.arena_offset += 1;
        self.arena.add(self.arena_offset)
    }

    fn arena_end(&self) -> *mut [u8; ALLOC_SIZE] {
        unsafe { self.arena.add(MAX_COUNT) }
    }

    pub unsafe fn offset_for(&self, ptr: *mut [u8; ALLOC_SIZE]) -> u32 {
        ptr.offset_from(self.arena) as u32
    }
}

impl<const ALLOC_SIZE: usize, const MAX_COUNT: usize> Heap for CheapHeap<ALLOC_SIZE, MAX_COUNT> {
    type MallocType = [AtomicMiniHeapId<Self>; ARENA_SIZE / PAGE_SIZE];
    type PointerType = AtomicMiniHeapId<Self>;
    unsafe fn map(
        &mut self,
        size: usize,
        flags: libc::c_int,
        fd: libc::c_int,
    ) -> Self::PointerType {
        let size = (size + PAGE_SIZE - 1) & (PAGE_SIZE - 1);
        let ptr = mmap(null_mut(), fd, size, 0).unwrap();
        AtomicMiniHeapId::new(ptr as *mut Self)
    }

    unsafe fn malloc(&mut self, size: usize) -> Self::MallocType {
        let addr = OneWayMmapHeap.malloc(size) as *mut Self;
        let mut page_data: [MaybeUninit<AtomicMiniHeapId<Self>>; ARENA_SIZE / PAGE_SIZE] =
            MaybeUninit::uninit().assume_init();

        (0..=(ARENA_SIZE / PAGE_SIZE)).for_each(|page| {
            let new_page = addr.add(page as usize);
            page_data[page].write(AtomicMiniHeapId::new(new_page));
        });

        std::mem::transmute::<_, Self::MallocType>(page_data)
    }

    unsafe fn get_size(&mut self, _: *mut ()) -> usize {
        ALLOC_SIZE
    }

    unsafe fn free(&mut self, ptr: *mut ()) {
        debug_assert_eq!(
            ptr.align_offset(std::mem::align_of::<[u8; ALLOC_SIZE]>()),
            0,
            "ptr must be aligned to our alloc size"
        );
        let ptr = ptr.cast::<[u8; ALLOC_SIZE]>();

        debug_assert!(
            (self.arena..self.arena_end()).contains(&ptr),
            "ptr must reside in our arena"
        );

        self.freelist.add(self.freelist_offset).write(ptr);
        self.freelist_offset += 1;
    }
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

    fn arena_end(&self) -> *mut u8 {
        unsafe { self.arena.add(self.alloc_size * self.max_count) }
    }
}

impl Heap for DynCheapHeap {
    type PointerType = *mut ();
    type MallocType = *mut ();

    unsafe fn map(&mut self, size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut () {
        todo!()
    }

    unsafe fn malloc(&mut self, size: usize) -> *mut () {
        OneWayMmapHeap.malloc(size)
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
