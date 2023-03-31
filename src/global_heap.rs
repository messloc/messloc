use core::{
    ptr::null_mut,
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    time::Duration,
};

use crate::{
    class_array::CLASS_ARRAY, comparatomic::Comparatomic, fake_std::dynarray::DynArray,
    meshable_arena::MeshableArena, mini_heap::MiniHeap, one_way_mmap_heap::OneWayMmapHeap,
    rng::Rng, shuffle_vector::ShuffleVector, MAX_SHUFFLE_VECTOR_LENGTH, MAX_SIZE, MAX_SMALL_SIZE,
    NUM_BINS, PAGE_SIZE,
};

pub struct GlobalHeap {
    pub arena: MeshableArena,
    pub shuffle_vector: DynArray<ShuffleVector<MAX_SHUFFLE_VECTOR_LENGTH>, NUM_BINS>,
    pub rng: Rng,
    pub last_mesh_effective: AtomicBool,
    pub mesh_period_ms: Duration,
    pub mini_heap_count: AtomicUsize,
    pub current: u64,
}

impl GlobalHeap {
    pub fn init() -> Self {
        let arena = MeshableArena::init();
        Self {
            arena,
            shuffle_vector: DynArray::create(),
            rng: Rng::init(),
            last_mesh_effective: AtomicBool::new(false),
            mesh_period_ms: Duration::new(0, 0),
            mini_heap_count: AtomicUsize::new(0),
            current: 0,
        }
    }

    /// Allocate a region of memory that can satisfy the requested bytes
    pub fn malloc(&mut self, bytes: usize) -> *const () {
        if let Some(size_class) = SizeMap.get_size_class(bytes) {
            let sv = match self.shuffle_vector.get(size_class) {
                Some(Some(s)) if let Some(sv) = unsafe { s.as_mut() } => {
                    sv
                },

                Some(None) => {
                    self.shuffle_vector.write_at(size_class, ShuffleVector::new());
                    unsafe { self.shuffle_vector.get(size_class).unwrap().unwrap().as_mut().unwrap() }
                    },

                _ => {
                        unreachable!()
                    }
                };

            let allocated = sv.malloc() as *mut ();

            //TODO:: Consider which strategy to pick - whether to allocate an entire page and
            //fragment or do each allocation separately
            if allocated.is_null() {
                let fresh = unsafe { OneWayMmapHeap.malloc(bytes) as *mut () };
                let _ = unsafe { self.arena.generate_mini_heap(fresh, bytes) };
                if self.arena.arena_begin.is_null() {
                    self.arena.arena_begin = fresh;
                }
                fresh
            } else {
                allocated
            }
        } else {
            unsafe { self.alloc_page_aligned(1).cast() }
        }
    }

    ///# Safety
    /// Unsafe

    pub unsafe fn free(&mut self, ptr: *mut (), bytes: usize) {
        if let Some(size_class) = SizeMap.get_size_class(bytes) {
            let shuffle_vectors = self.shuffle_vector.as_mut_slice().as_mut().unwrap();
            let mh = self.arena.get_mini_heap(ptr).unwrap();
            match shuffle_vectors.get(size_class) {
                Some(Some(sv)) if let Some(v) = sv.as_mut() => {
                    let mini_heaps = v.mini_heaps.as_mut_slice().as_mut().unwrap();
                    let pos = mini_heaps.iter().position(|mh| mh.is_none()).unwrap();

                    let _ = core::mem::replace(&mut mini_heaps[pos], Some(mh));
                }
                _ => {}
            }
        } else {
            unreachable!()
        }
    }

    /// Allocate the requested number of pages
    unsafe fn alloc_page_aligned(&mut self, page_count: usize) -> *mut MiniHeap {
        // if given a very large allocation size (e.g. (usize::MAX)-8), it is possible
        // the pages calculation overflowed. An allocation that big is impossible
        // to satisfy anyway, so just fail early.
        if page_count == 0 {
            return null_mut();
        }

        self.alloc_miniheap(page_count)

        //   d_assert(mh->isLargeAlloc());
        //   d_assert(mh->spanSize() == pageCount * kPageSize);
        //   // d_assert(mh->objectSize() == pageCount * kPageSize);
        //
    }

    fn alloc_miniheap(&mut self, page_count: usize) -> *mut MiniHeap {
        debug_assert!(page_count > 0, "should allocate at least 1 page");

        let page = unsafe { OneWayMmapHeap.malloc(PAGE_SIZE) } as *mut ();

        let buf =
            unsafe { OneWayMmapHeap.malloc(core::mem::size_of::<MiniHeap>()) } as *mut MiniHeap;
        // allocate out of the arena
        // TODO: Check if we need this since it doesn't match the current lazy model

        //TODO: Adjust value of span by going through find_pages on the arena and the related code
        unsafe {
            buf.write(MiniHeap::new(page, PAGE_SIZE));
        }

        // // mesh::debug("%p (%u) created!\n", mh, GetMiniHeapID(mh));

        self.mini_heap_count.fetch_add(1, Ordering::AcqRel);
        buf
    }
}

unsafe impl Send for GlobalHeap {}

trait Meshable {
    fn is_meshable(&self, other: &Self) -> bool;
}

impl<const N: usize> Meshable for [Comparatomic<AtomicU64>; N] {
    fn is_meshable(&self, other: &Self) -> bool {
        self.iter().zip(other.iter()).fold(0, |mut acc, (lb, rb)| {
            acc |= lb.load(Ordering::SeqCst) & rb.load(Ordering::SeqCst);
            acc
        }) == 0
    }
}

struct SizeMap;

impl SizeMap {
    pub fn get_size_class(&self, size: usize) -> Option<usize> {
        let idx = self.class_index_maybe(size)?;
        Some(CLASS_ARRAY[idx] as usize)
    }

    #[allow(clippy::unused_self)]
    const fn class_index_maybe(&self, size: usize) -> Option<usize> {
        // this is overlapping but allowed because it currently is the nicest way
        // to write `MAX_SMALL_SIZE+1..MAX_SIZE`
        #[allow(clippy::match_overlapping_arm)]
        match size {
            0..=MAX_SMALL_SIZE => Some((size + 7) >> 3),
            ..=MAX_SIZE => Some((size + 127 + (120 << 7)) >> 7),
            _ => None,
        }
    }
}
