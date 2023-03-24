use core::{
    assert_matches::assert_matches,
    cell::RefCell,
    cell::{Ref, RefMut},
    mem::{size_of, MaybeUninit},
    ops::{Deref, DerefMut},
    ptr::{addr_of_mut, null, null_mut},
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    time::Duration,
};

use arrayvec::ArrayVec;
use spin::{Mutex, MutexGuard};

use crate::{
    bitmap::BitmapBase,
    cheap_heap::CheapHeap,
    class_array::CLASS_ARRAY,
    comparatomic::Comparatomic,
    fake_std::dynarray::DynArray,
    flags::FreeListId,
    for_each_meshed,
    list_entry::ListEntry,
    meshable_arena::{MeshableArena, PageType},
    mini_heap::{self, MiniHeap, MiniHeapId},
    one_way_mmap_heap::{Heap, OneWayMmapHeap},
    rng::Rng,
    runtime::{FastWalkTime, FreeList, Messloc},
    shuffle_vector::{self, ShuffleVector},
    span::Span,
    splits::{MergeElement, MergeSetWithSplits, SplitType},
    ARENA_SIZE, BINNED_TRACKER_MAX_EMPTY, MAX_MERGE_SETS, MAX_MESHES, MAX_MESHES_PER_ITERATION,
    MAX_MINI_HEAPS_PER_SHUFFLE_VECTOR, MAX_SHUFFLE_VECTOR_LENGTH, MAX_SIZE, MAX_SMALL_SIZE,
    MAX_SPLIT_LIST_SIZE, MINI_HEAP_REFILL_GOAL_SIZE, MIN_OBJECT_SIZE, MIN_STRING_LEN, NUM_BINS,
    OCCUPANCY_CUTOFF, PAGE_SIZE,
};

#[allow(clippy::module_name_repetitions)]
#[derive(Default)]
pub struct GlobalHeapStats {
    mesh_count: AtomicUsize,
    alloc_count: AtomicUsize,
    high_water_mark: AtomicUsize,
}

pub struct GlobalHeap {
    pub arena: MeshableArena,
    pub shuffle_vector: DynArray<ShuffleVector<MAX_SHUFFLE_VECTOR_LENGTH>, NUM_BINS>,
    pub free_lists: *mut FreeList,
    pub merge_set: *mut (),
    pub mesh_epoch: Epoch,
    pub rng: Rng,
    pub last_mesh_effective: AtomicBool,
    pub mesh_period_ms: Duration,
    pub stats: GlobalHeapStats,
    pub mini_heap_count: AtomicUsize,
    pub current: u64,
}

/// Returns the minimum number of pages needed to
/// hold the requested allocation
const fn page_count(bytes: usize) -> usize {
    // bytes.div_ceil(PAGE_SIZE)
    (bytes.wrapping_add(PAGE_SIZE - 1)) / PAGE_SIZE
}

impl GlobalHeap {
    pub fn init() -> Self {
        let size = core::mem::size_of::<[ShuffleVector<MAX_SHUFFLE_VECTOR_LENGTH>; NUM_BINS]>();

        let arena = MeshableArena::init();
        let merge_set = MergeSetWithSplits::<MAX_SPLIT_LIST_SIZE>::alloc_new();
        Self {
            arena,
            shuffle_vector: DynArray::create(),
            free_lists: FreeList::alloc_new(),
            merge_set,
            mesh_epoch: Epoch::default(),
            rng: Rng::init(),
            last_mesh_effective: AtomicBool::new(false),
            mesh_period_ms: Duration::new(0, 0),
            stats: GlobalHeapStats::default(),
            mini_heap_count: AtomicUsize::new(0),
            current: 0,
        }
    }

    /// Allocate a region of memory that can satisfy the requested bytes
    pub fn malloc(&mut self, bytes: usize) -> *const () {
        if let Some(size_class) = SizeMap.get_size_class(bytes) {
            let sv = match self.shuffle_vector.get(size_class) {
                Some(Some(s)) if let Some(sv) = unsafe { s.as_mut() } => {
                    if sv.is_exhausted_and_no_refill() {
                        self.small_alloc_global_refill(size_class);
                    }
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
            unsafe { self.alloc_page_aligned(1, page_count(bytes)).cast() }
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

                    core::mem::replace(&mut mini_heaps[pos], Some(mh));
                }
                _ => {}
            }
        } else {
            unreachable!()
        }
    }

    fn small_alloc_global_refill(&mut self, size_class: usize) {
        let size_max = SizeMap.bytes_size_for_class(size_class);
        let current = self.current;

        self.small_alloc_mini_heaps(size_class, size_max, current);
    }

    /// Allocate the requested number of pages
    unsafe fn alloc_page_aligned(&mut self, page_align: usize, page_count: usize) -> *mut MiniHeap {
        // if given a very large allocation size (e.g. (usize::MAX)-8), it is possible
        // the pages calculation overflowed. An allocation that big is impossible
        // to satisfy anyway, so just fail early.
        if page_count == 0 {
            return null_mut();
        }

        self.alloc_miniheap(page_count, page_align)

        //   d_assert(mh->isLargeAlloc());
        //   d_assert(mh->spanSize() == pageCount * kPageSize);
        //   // d_assert(mh->objectSize() == pageCount * kPageSize);
        //
    }

    unsafe fn free_for(&mut self, heap: *mut MiniHeap, offset: usize, mut epoch: Epoch) {
        epoch.set(self.mesh_epoch.current(), Ordering::AcqRel);

        let mini_heap = unsafe { heap.as_mut().unwrap() };

        if mini_heap.is_large_alloc() {
            self.free_mini_heap_locked(mini_heap as *const _ as *mut (), false);
        } else {
            assert!(mini_heap.max_count() > 1);

            self.last_mesh_effective.compare_exchange(
                false,
                true,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            let remaining = mini_heap.in_use_count() - 1;
            let was_set = mini_heap.clear_if_not_free(offset);
            let ptr = unsafe { self.arena.arena_begin.cast::<MiniHeap>().add(offset) as *mut () };

            let mh = {
                let offset = unsafe {
                    usize::try_from(ptr.offset_from(self.arena.arena_begin as *mut ())).unwrap()
                };
                unsafe {
                    if let MiniHeapId::HeapPointer(ptr) =
                        *self.arena.mh_allocator.cast::<MiniHeapId>().add(offset)
                    {
                        ptr.as_mut().unwrap()
                    } else {
                        unreachable!()
                    }
                }
            };

            if epoch.0.load(Ordering::Acquire) % 2 == 1 || !self.mesh_epoch.is_same(&epoch) {
                if self.is_related(ptr.cast(), mini_heap as *const _ as *mut ()) && was_set {
                    let size_class = mh.size_class();
                    assert_eq!(mini_heap.size_class(), mh.size_class());
                    unsafe { mh.free(ptr as *mut ()) };

                    if mh.is_attached()
                        && (mh.in_use_count() == 0 || mh.free_list_id() == FreeListId::Full)
                    {
                        if self.post_free_locked(heap, 0).is_some() {
                            self.flush_bin_locked(size_class as usize);
                        }
                    } else {
                        self.maybe_mesh();
                    }
                }
            } else if mini_heap.is_attached()
                && (remaining == 0 || mini_heap.free_list_id() == FreeListId::Full)
            {
                let size_class = mh.size_class();
                if mh != mini_heap {
                    if self.is_related(mh as *const _ as *mut (), mini_heap as *const _ as *mut ())
                    {
                        // TODO: store created_epoch on mh and check it here
                    }
                } else if self.post_free_locked(heap, mh.in_use_count()).is_some() {
                    self.flush_bin_locked(size_class as usize);
                }
            } else {
                self.maybe_mesh();
            }
        }
    }

    pub fn is_related(&self, mut mh: *mut (), other: *mut ()) -> bool {
        let mini_heap = unsafe { mh.cast::<MiniHeap>().as_mut().unwrap() };
        crate::for_each_meshed!(mini_heap {
            mini_heap as *const _ as *mut () == other

        })
    }

    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn mini_heap_for_with_epoch(
        &self,
        ptr: *mut (),
        epoch: &mut Epoch,
    ) -> *mut MiniHeap {
        epoch.set(self.mesh_epoch.current(), Ordering::Acquire);

        let offset = ptr.offset_from(self.arena.arena_begin as *mut ()) as usize;
        if let MiniHeapId::HeapPointer(ptr) = self
            .arena
            .mh_allocator
            .cast::<MiniHeapId>()
            .add(offset)
            .read()
        {
            ptr
        } else {
            unreachable!()
        }
    }

    #[allow(clippy::needless_for_each)]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn free_mini_heap_locked(&mut self, mini_heap: *mut (), untrack: bool) {
        let addr = self.arena.arena_begin;

        let mut to_free: [MaybeUninit<*mut MiniHeap>; MAX_MESHES] = MaybeUninit::uninit_array();

        let mh = unsafe { mini_heap.cast::<MiniHeap>().as_mut().unwrap() };
        let mut last = 0;

        crate::for_each_meshed!(mh {
            to_free[last].write(mh);
            last += 1;
            false
        });

        let to_free = unsafe { MaybeUninit::array_assume_init(to_free) };

        let begin = self.arena.arena_begin;

        to_free.iter().for_each(|heap| {
            let mh = unsafe { heap.as_mut().unwrap() };
            let mh_type = if mh.is_meshed() {
                PageType::Meshed
            } else {
                PageType::Dirty
            };
            let span_start = mh.span_start;
            self.pfree(span_start as *mut ());

            if untrack && !mh.is_meshed() {
                self.untrack_mini_heap_locked(mini_heap);
            }
            let allocator = self
                .arena
                .mh_allocator
                .cast::<CheapHeap<64, { ARENA_SIZE / PAGE_SIZE }>>()
                .as_mut()
                .unwrap();
            allocator.free(mini_heap);

            self.mini_heap_count.fetch_sub(1, Ordering::AcqRel);
        });
    }

    pub fn untrack_mini_heap_locked(&self, mut mh: *mut ()) {
        self.stats.alloc_count.fetch_add(1, Ordering::AcqRel);
        let freelist = unsafe { &self.free_lists.as_mut().unwrap().0 };

        let miniheap = unsafe { mh.cast::<MiniHeap>().as_mut().unwrap() };
        let size_class = miniheap.size_class() as usize;

        let mut list = match &miniheap.free_list_id() {
            FreeListId::Empty => Some(&freelist[0][size_class].0),
            FreeListId::Full => Some(&freelist[1][size_class].0),
            FreeListId::Partial => Some(&freelist[2][size_class].0),
            _ => None,
        }
        .unwrap() as *const _ as *mut ListEntry;

        miniheap.free_list.remove(list)
    }

    #[allow(clippy::missing_safety_doc)]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn pfree(&mut self, ptr: *mut ()) {
        let mut start_epoch = Epoch::default();
        let offset = unsafe { ptr.offset_from(self.arena.arena_begin as *mut ()) };

        self.free_for(
            ptr as *mut MiniHeap,
            usize::try_from(offset).unwrap(),
            start_epoch,
        );
    }

    pub fn maybe_mesh(&mut self) {
        self.mesh_all_sizes_mesh_locked();
    }

    pub fn mesh_all_sizes_mesh_locked(&mut self) {
        // TODO:: add assert checks if needed

        self.arena.scavenge(true);

        if self.last_mesh_effective.load(Ordering::Acquire) && self.arena.above_mesh_threshold() {
            self.flush_all_bins();

            let total_mesh_count = (0..NUM_BINS)
                .map(|sz| self.mesh_size_class_locked(sz))
                .sum();

            unsafe {
                let merge_sets = unsafe {
                    self.merge_set
                        .cast::<MergeSetWithSplits<MAX_SPLIT_LIST_SIZE>>()
                        .as_mut()
                        .unwrap()
                };
                merge_sets.madvise()
            };

            self.last_mesh_effective
                .store(total_mesh_count > 256, Ordering::Release);
            self.stats
                .mesh_count
                .fetch_add(total_mesh_count, Ordering::Acquire);

            self.arena.scavenge(true);
        }
    }

    pub fn small_alloc_mini_heaps(&mut self, size_class: usize, object_size: usize, current: u64) {
        (0..NUM_BINS).for_each(|bin| unsafe {
            match self
                .shuffle_vector
                .get(bin) {
                    Some(Some(v)) if let Some(sv) = v.as_mut() => {
                        let mini_heaps = sv.mini_heaps.as_mut_slice().as_mut().unwrap();

                mini_heaps.iter().filter_map(|x| *x).for_each(|mini_heap| {
                let mini_heap = mini_heap.as_mut().unwrap();
                mini_heap.unset_attached();
                self.post_free_locked(mini_heap, mini_heap.in_use_count());
            });
                    },

                    _ => unreachable!(),
                }
        });
        assert!(size_class < NUM_BINS);

        let (mut mini_heaps, mut bytes_free) = self.select_for_reuse(size_class, current);

        if bytes_free < MINI_HEAP_REFILL_GOAL_SIZE && !mini_heaps.len() == MAX_SPLIT_LIST_SIZE {
            let object_count = MIN_STRING_LEN.max(PAGE_SIZE / object_size);
            let page_count = page_count(object_size * object_count);
            let mut count = 0;

            while bytes_free < MINI_HEAP_REFILL_GOAL_SIZE
                && !mini_heaps.len() == MAX_SPLIT_LIST_SIZE
            {
                let mut mh = self.alloc_mini_heap_locked(page_count, object_count, object_size, 1);
                let mut heap = unsafe { mh.as_mut().unwrap() };
                assert!(heap.is_attached());

                heap.set_attached(current, &heap.free_list as *const _ as *mut ListEntry);
                assert!(heap.is_attached() && heap.current() == current);
                mini_heaps[count] = mh;
                count += 1;
                bytes_free += heap.bytes_free();
            }
        }
    }

    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn release_mini_heap_locked(&mut self, mini_heap: *mut MiniHeap) {
        let mini_heap = mini_heap.as_mut().unwrap();
        mini_heap.unset_attached();
        self.post_free_locked(mini_heap, mini_heap.in_use_count());
    }

    pub fn set_mesh_period_ms(&mut self, period: Duration) {
        self.mesh_period_ms = period;
    }

    pub fn flush_all_bins(&mut self) {
        (0..NUM_BINS).for_each(|bin| {
            self.flush_bin_locked(bin);
        });
    }

    pub fn flush_bin_locked(&mut self, size_class: usize) {
        let free_list = unsafe { &self.free_lists.as_mut().unwrap() };
        let mut next = &free_list.0[0][size_class].0.next;

        while let MiniHeapId::HeapPointer(next_id) = next {
            let mut mh = unsafe { next_id.as_mut().unwrap() };
            next = unsafe { &mh.free_list.next };

            unsafe { self.free_mini_heap_locked(next_id as *const _ as *mut (), true) };
            free_list.0[0][size_class].1.store(1, Ordering::Acquire);
        }

        unsafe {
            assert_matches!(free_list.0[0][size_class].0.next, MiniHeapId::Head);

            assert_matches!(free_list.0[0][size_class].0.prev, MiniHeapId::Head);
        }
    }

    pub fn mesh_size_class_locked(&mut self, size_class: usize) -> usize {
        let merge_set_count = self.shifted_splitting(size_class);
        //TODO: remove allocation
        let mut merge_set: Vec<(*mut MiniHeap, *mut MiniHeap)> = {
            unsafe {
                self.merge_set
                    .cast::<MergeSetWithSplits<MAX_SPLIT_LIST_SIZE>>()
                    .as_mut()
                    .unwrap()
                    .0
                    .iter()
                    .filter_map(|merge| match merge {
                        MergeElement {
                            mini_heap: dst,
                            direction: SplitType::MergedWith(src),
                        } => Some((*dst, *src)),
                        _ => None,
                    })
                    .collect()
            }
        };

        let mesh_count = merge_set
            .into_iter()
            .try_fold(0, |mut mesh_count, (mut dst, mut src)| {
                let (src_obj, dst_obj) = unsafe { (src.as_mut().unwrap(), dst.as_mut().unwrap()) };

                let dst_count = dst_obj.mesh_count();
                let src_count = src_obj.mesh_count();
                if dst_count + src_count <= MAX_MESHES {
                    if dst_count < src_count {
                        core::mem::swap(&mut dst, &mut src);
                    }

                    unsafe {
                        match (dst_obj.in_use_count(), src_obj.in_use_count()) {
                            (0, 0) => {
                                self.post_free_locked(dst, 0).unwrap();
                                self.post_free_locked(src, 0).unwrap();
                            }
                            (0, _) => {
                                self.post_free_locked(dst, 0).unwrap();
                            }

                            (_, 0) => {
                                self.post_free_locked(src, 0);
                            }
                            _ => {
                                unsafe { self.mesh_locked(dst, src) };
                                mesh_count += 1;
                            }
                        }
                    }
                }
                Some(mesh_count)
            });

        self.flush_bin_locked(size_class);
        mesh_count.unwrap()
    }

    pub fn select_for_reuse(
        &self,
        size_class: usize,
        current: u64,
    ) -> ([*mut MiniHeap; MAX_SPLIT_LIST_SIZE], usize) {
        let free_list = unsafe { self.free_lists.as_mut().unwrap() };
        let (mut mini_heaps, mut bytes_free) =
            self.fill_from_list(current, &free_list.0[2][size_class]);
        if bytes_free < MINI_HEAP_REFILL_GOAL_SIZE {
            // TODO:: check if we need to append to previous list or not
            let (mini_heaps, bytes) = self.fill_from_list(current, &free_list.0[0][size_class]);
            bytes_free += bytes;
        }
        (mini_heaps, bytes_free)
    }

    pub fn fill_from_list(
        &self,
        current: u64,
        free_list: &(ListEntry, Comparatomic<AtomicU64>),
    ) -> ([*mut MiniHeap; MAX_SPLIT_LIST_SIZE], usize) {
        let mut next = &free_list.0.next;
        let mut next_id = if let MiniHeapId::HeapPointer(p) = next {
            p
        } else {
            unreachable!()
        };
        let mut bytes_free = 0;

        let mut mini_heaps = core::array::from_fn(|_| null_mut());
        let mut count = 0;
        while let MiniHeapId::HeapPointer(mh) = next && bytes_free < MINI_HEAP_REFILL_GOAL_SIZE {
            let mut heap = unsafe {
                mh.as_mut().unwrap()
            };

            let mut heap = unsafe { mh.as_mut().unwrap() };
            let next = unsafe { &heap.free_list.next };
            //TODO: remove if removing it causes better experience as discovered in the original
            //source
            bytes_free += heap.bytes_free();
            assert!(heap.is_attached());
            let freelist = unsafe { &self.free_lists.as_mut().unwrap() };
            let size_class = heap.size_class() as usize;
            let fl = match &heap.free_list_id() {
                FreeListId::Empty => Some(&freelist.0[0][size_class].0),
                FreeListId::Full => Some(&freelist.0[1][size_class].0),
                FreeListId::Partial => Some(&freelist.0[2][size_class].0),
                _ => None,
            }
            .unwrap();

            heap.set_attached(current, fl as *const _ as *mut ListEntry);
            mini_heaps[count] = (mh as *const _ as *mut MiniHeap);
            count += 1;
            free_list.1.inner().fetch_sub(1, Ordering::AcqRel);
        }

        (mini_heaps, bytes_free)
    }

    pub fn shifted_splitting(&mut self, size_class: usize) -> Option<usize> {
        let free_lists = unsafe { &self.free_lists.as_mut().unwrap() };
        let mh = &free_lists.0[0].get(size_class).unwrap().0;
        let (left, right) = self.half_split(size_class);
        let (mut lc, mut rc) = (0usize, 0usize);
        let mut left_set: [*mut MiniHeap; MAX_SPLIT_LIST_SIZE] =
            core::array::from_fn(|_| null_mut());
        let mut right_set: [*mut MiniHeap; MAX_SPLIT_LIST_SIZE] =
            core::array::from_fn(|_| null_mut());
        unsafe {
            self.merge_set
                .cast::<MergeSetWithSplits<MAX_SPLIT_LIST_SIZE>>()
                .as_mut()
                .unwrap()
        }
        .0
        .iter()
        .filter(|split| {
            !matches!(
                split,
                MergeElement {
                    direction: SplitType::MergedWith(_),
                    ..
                }
            )
        })
        .for_each(|merge| {
            let MergeElement {
                mini_heap,
                direction,
            } = merge;
            match direction {
                SplitType::Left => {
                    left_set[lc] = *mini_heap;
                    lc += 1;
                }
                SplitType::Right => {
                    right_set[rc] = *mini_heap;
                    rc += 1;
                }
                _ => {}
            }
        });

        Some((0..left).fold(0, |mut count, j| {
            let mut idx_right = j;
            count += (0..right.min(64))
                .scan((0, 0), |(mut count, mut found_count), i| {
                    let bitmap1 = unsafe { &left_set.get(j).unwrap().as_mut().unwrap().bitmap };
                    let bitmap2 = unsafe { &right_set.get(j).unwrap().as_mut().unwrap().bitmap };

                    let is_meshable = bitmap1
                        .internal_type
                        .bits()
                        .iter()
                        .zip(bitmap2.internal_type.bits().iter())
                        .fold(0u64, |mut acc, (lb, rb)| {
                            acc |= lb & rb;
                            acc
                        });

                    if is_meshable == 0 {
                        found_count += 1;

                        left_set[j] = null_mut();
                        right_set[idx_right] = null_mut();
                        idx_right += 1;
                        let merge_set_count = self.mesh_found(&left_set[..], &right_set[..]);

                        if found_count > MAX_MESHES_PER_ITERATION || count < MAX_MERGE_SETS {
                            None
                        } else {
                            Some((count, found_count))
                        }
                    } else {
                        Some((count, found_count))
                    }
                })
                .fold(0, |count, _| count);
            count
        }))
    }

    pub fn half_split(&mut self, size_class: usize) -> (usize, usize) {
        let lists = unsafe { &self.free_lists.as_mut().unwrap().0 };
        let mut next = &lists[2][size_class].0.next;

        let mut left_size = 0usize;
        let mut right_size = 0usize;
        let mut last_added = 0;

        while let MiniHeapId::HeapPointer(mh_id) = next && left_size < MAX_SPLIT_LIST_SIZE && right_size < MAX_SPLIT_LIST_SIZE
        {
            let mh = unsafe {
                mh_id.as_mut().unwrap()
            };
            next = &mh.free_list.next;
            if mh.is_meshing_candidate() || mh.fullness() >= OCCUPANCY_CUTOFF {
                let (index, mut free) = unsafe { self.merge_set.cast::<MergeSetWithSplits<MAX_SPLIT_LIST_SIZE>>().as_mut().unwrap().0.iter().enumerate().find(|(key, ele)| ele.mini_heap.is_null()).unwrap() };
                let direction = if left_size <= right_size { left_size +=1; SplitType::Left } else { right_size += 1; SplitType::Right };
                    free = &MergeElement {
                        mini_heap: mh as *const _ as *mut MiniHeap,
                        direction,
                    };

                    last_added = index;
            }
        }

        let mut rng = &self.rng;
        let set = unsafe {
            self.merge_set
                .cast::<MergeSetWithSplits<MAX_SPLIT_LIST_SIZE>>()
                .as_mut()
                .unwrap()
        };
        unsafe { self.rng.shuffle(&mut set.0, 0, last_added) };
        (left_size, right_size)
    }

    pub fn mesh_found(&mut self, left: &[*mut MiniHeap], right: &[*mut MiniHeap]) -> usize {
        let merge_set_count = unsafe {
            left.iter()
                .enumerate()
                .zip(right.iter())
                .fold(0, |mut acc, ((key, le), ri)| {
                    if le.as_mut().unwrap().is_meshing_candidate()
                        && ri.as_mut().unwrap().is_meshing_candidate()
                    {
                        self.merge_set
                            .cast::<MergeSetWithSplits<MAX_SPLIT_LIST_SIZE>>()
                            .as_mut()
                            .unwrap()
                            .0[key]
                            .direction = SplitType::MergedWith(*ri);
                    }
                    acc += 1;
                    acc
                })
        };
        merge_set_count
    }

    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn post_free_locked(&mut self, mh: *mut MiniHeap, in_use: usize) -> Option<()> {
        let mini_heap = mh.as_mut().unwrap();
        mini_heap.is_attached().then_some(())?;
        let mut free_lists = unsafe { &mut self.free_lists.as_mut().unwrap() };
        let size_class = mini_heap.size_class() as usize;

        let current_free_list = match &mini_heap.free_list_id() {
            FreeListId::Empty => Some(&free_lists.0[0][size_class].0),
            FreeListId::Full => Some(&free_lists.0[1][size_class].0),
            FreeListId::Partial => Some(&free_lists.0[2][size_class].0),
            _ => None,
        };

        let current_free_list = current_free_list.unwrap() as *const _ as *mut ListEntry;
        let mut free_list_id = mini_heap.free_list_id();
        let max_count = usize::try_from(mini_heap.max_count()).unwrap();
        let size_class = mini_heap.size_class() as usize;

        let (new_list_id, mut list) = match (in_use, free_list_id) {
            (0, FreeListId::Empty) => return None,
            (iu, FreeListId::Full) if iu == max_count => return None,
            (0, _) => (FreeListId::Empty, &mut free_lists.0[0][size_class]),
            (iu, _) if iu == max_count => (FreeListId::Full, &mut free_lists.0[1][size_class]),
            (_, FreeListId::Partial) => return None,
            _ => (FreeListId::Partial, &mut free_lists.0[2][size_class]),
        };
        list.0.add(
            current_free_list,
            new_list_id as u32,
            MiniHeapId::None,
            mh as *mut (),
        );

        list.1.fetch_add(1, Ordering::AcqRel);

        let empties = free_lists.0[0]
            .get(size_class)
            .unwrap()
            .1
            .load(Ordering::Acquire);

        (empties > BINNED_TRACKER_MAX_EMPTY).then_some(())
    }

    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn mesh_locked(&self, dst: *mut MiniHeap, src: *mut MiniHeap) {
        let dst = dst.as_mut().unwrap();

        let src = src.as_mut().unwrap();

        crate::for_each_meshed!(src {
            let src_span = src.span_start as *mut libc::c_void;
            self.arena
                .begin_mesh(src_span, dst.span_size());
            false
        });

        dst.consume(src);
    }

    pub fn page_aligned_alloc(&mut self, alignment: usize, size: usize) -> *mut () {
        let page_count = page_count(size);
        let mut mh = unsafe {
            self.alloc_mini_heap_locked(page_count, 1, page_count * PAGE_SIZE, alignment)
                .as_mut()
                .unwrap()
        };

        assert!(mh.is_large_alloc());
        assert!(mh.span_size() == page_count * PAGE_SIZE);

        unsafe { mh.malloc_at(0) }
    }

    pub fn alloc_mini_heap_locked(
        &mut self,
        page_count: usize,
        object_count: usize,
        object_size: usize,
        alignment: usize,
    ) -> *mut MiniHeap {
        let buffer = unsafe {
            self.arena
                .mh_allocator
                .cast::<CheapHeap<64, { ARENA_SIZE / PAGE_SIZE }>>()
                .as_mut()
                .unwrap()
                .alloc()
        } as *mut MiniHeap;
        let span = Span::default();
        let span_begin = self.arena.page_alloc(page_count, alignment);
        let mut mh = MiniHeap::with_object(span.clone(), object_count, object_size);
        let mini_heap_id = unsafe { buffer.offset_from(self.arena.arena_begin as *const _) };

        (0..span.length).for_each(|i| unsafe {
            self.arena
                .mh_allocator
                .cast::<MiniHeapId>()
                .add(span.offset + i)
                .write(MiniHeapId::HeapPointer(buffer));
        });

        self.mini_heap_count.fetch_add(1, Ordering::Acquire);
        self.stats.alloc_count.fetch_add(1, Ordering::AcqRel);
        let hwm = self.stats.high_water_mark.load(Ordering::Acquire);
        let count = self.mini_heap_count.load(Ordering::Acquire);
        if count > hwm {
            self.stats.high_water_mark.store(count, Ordering::Release);
        }
        addr_of_mut!(mh)
    }

    fn alloc_miniheap(&mut self, page_count: usize, page_align: usize) -> *mut MiniHeap {
        debug_assert!(page_count > 0, "should allocate at least 1 page");

        let page = unsafe { OneWayMmapHeap.malloc(PAGE_SIZE) } as *mut ();

        let buf =
            unsafe { OneWayMmapHeap.malloc(core::mem::size_of::<MiniHeap>()) } as *mut MiniHeap;
        // allocate out of the arena
        // TODO: Check if we need this since it doesn't match the current lazy model
        // let (span, span_begin) = self.arena.page_alloc(page_count, page_align);

        //TODO: Adjust value of span by going through find_pages on the arena and the related code
        unsafe {
            buf.write(MiniHeap::new(page, Span::default(), PAGE_SIZE));
        }

        // // mesh::debug("%p (%u) created!\n", mh, GetMiniHeapID(mh));

        self.mini_heap_count.fetch_add(1, Ordering::AcqRel);
        self.stats.alloc_count.fetch_add(1, Ordering::AcqRel);
        let count = self.mini_heap_count.load(Ordering::Acquire);
        self.stats.high_water_mark.store(count, Ordering::Release);
        buf
    }
}

unsafe impl Send for GlobalHeap {}

#[derive(Default, PartialEq)]
pub struct Epoch(Comparatomic<AtomicU64>);

impl Epoch {
    pub fn current(&self) -> u64 {
        self.0.inner().load(Ordering::SeqCst)
    }

    pub fn is_same(&self, start_epoch: &Self) -> bool {
        self == start_epoch
    }

    pub fn set(&self, value: u64, ordering: Ordering) {
        self.0.inner().store(value, ordering);
    }
}
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

#[derive(Clone, Debug)]
pub enum List {
    Full,
    Partial,
    Empty,
    Attached,
    Max,
}

struct SizeMap;

impl SizeMap {
    pub fn get_size_class(&self, size: usize) -> Option<usize> {
        let idx = self.class_index_maybe(size)?;
        Some(CLASS_ARRAY[idx] as usize)
    }

    pub const fn class_index_maybe(&self, size: usize) -> Option<usize> {
        // this is overlapping but allowed because it currently is the nicest way
        // to write `MAX_SMALL_SIZE+1..MAX_SIZE`
        #[allow(clippy::match_overlapping_arm)]
        match size {
            0..=MAX_SMALL_SIZE => Some((size + 7) >> 3),
            ..=MAX_SIZE => Some((size + 127 + (120 << 7)) >> 7),
            _ => None,
        }
    }

    pub const fn bytes_size_for_class(&self, size: usize) -> usize {
        1 << (size + crate::utils::stlog(MIN_OBJECT_SIZE))
    }
}
