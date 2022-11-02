use std::{
    assert_matches::assert_matches,
    cell::RefCell,
    cell::{Ref, RefMut},
    mem::{size_of, MaybeUninit},
    ops::{Deref, DerefMut},
    ptr::{addr_of_mut, null, null_mut},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard, PoisonError,
    },
    time::{Duration, SystemTime},
};

use arrayvec::ArrayVec;

use crate::{
    atomic_enum::AtomicOption,
    bitmap::BitmapBase,
    cheap_heap::CheapHeap,
    comparatomic::Comparatomic,
    for_each_meshed,
    list_entry::ListEntry,
    meshable_arena::{MeshableArena, PageType},
    mini_heap::{self, FreeListId, MiniHeap, MiniHeapId},
    one_way_mmap_heap::Heap,
    rng::Rng,
    runtime::{self, FastWalkTime, FreeList, Messloc},
    shuffle_vector::{self, ShuffleVector},
    span::Span,
    BINNED_TRACKER_MAX_EMPTY, MAX_MERGE_SETS, MAX_MESHES, MAX_MESHES_PER_ITERATION,
    MAX_SPLIT_LIST_SIZE, MINI_HEAP_REFILL_GOAL_SIZE, MIN_STRING_LEN, NUM_BINS, OCCUPANCY_CUTOFF,
    PAGE_SIZE,
};

#[allow(clippy::module_name_repetitions)]
#[derive(Default)]
pub struct GlobalHeapStats {
    mesh_count: AtomicUsize,
    alloc_count: AtomicUsize,
    high_water_mark: AtomicUsize,
}

pub struct GlobalHeap {
    runtime: Messloc,
    pub arena: Mutex<MeshableArena>,
    last_mesh_effective: AtomicBool,
    mesh_epoch: Epoch,
    pub free_lists: FreeList,
    access_lock: Mutex<()>,
    rng: Rng,
    mesh_period_ms: Duration,
    stats: GlobalHeapStats,
    pub mini_heap_count: AtomicUsize,
}

/// Returns the minimum number of pages needed to
/// hold the requested allocation
const fn page_count(bytes: usize) -> usize {
    // bytes.div_ceil(PAGE_SIZE)
    (bytes.wrapping_add(PAGE_SIZE - 1)) / PAGE_SIZE
}

impl GlobalHeap {
    pub fn init(mut runtime: MaybeUninit<FastWalkTime>) -> Messloc {
        let runtime_ptr = runtime.as_mut_ptr();

        let arena = MeshableArena::init();

        let mesh_period = std::env::var("MESH_PERIOD_MS").unwrap();
        let period = mesh_period.parse().unwrap();

        let mut heap = MaybeUninit::<GlobalHeap>::uninit();
        let ptr = heap.as_mut_ptr();
        unsafe {
            addr_of_mut!((*ptr).arena).write(Mutex::new(arena));
            addr_of_mut!((*ptr).stats).write(GlobalHeapStats::default());
            addr_of_mut!((*ptr).last_mesh_effective).write(AtomicBool::new(false));
            addr_of_mut!((*ptr).mini_heap_count).write(AtomicUsize::default());
            addr_of_mut!((*ptr).last_mesh_effective).write(AtomicBool::new(false));
            addr_of_mut!((*ptr).free_lists).write(FreeList::init());
            addr_of_mut!((*ptr).mesh_epoch).write(Epoch::default());
            addr_of_mut!((*ptr).rng).write(Rng::init());
            addr_of_mut!((*ptr).mesh_period_ms).write(Duration::from_millis(period));
        }
        let heap = unsafe { heap.assume_init() };

        unsafe {
            addr_of_mut!((*runtime_ptr).global_heap).write(heap);
        }

        unsafe { Messloc(Arc::new(Mutex::new(runtime.assume_init()))) }
    }

    /// Allocate a region of memory that can satisfy the requested bytes
    pub fn malloc(&mut self, bytes: usize) -> *const () {
        self.alloc_page_aligned(1, page_count(bytes))
    }

    /// Allocate the requested number of pages
    fn alloc_page_aligned(&mut self, page_align: usize, page_count: usize) -> *const () {
        // if given a very large allocation size (e.g. (usize::MAX)-8), it is possible
        // the pages calculation overflowed. An allocation that big is impossible
        // to satisfy anyway, so just fail early.
        if page_count == 0 {
            return null();
        }

        let miniheap = self.alloc_miniheap(page_count, 1, page_count * PAGE_SIZE, page_align);

        //   d_assert(mh->isLargeAlloc());
        //   d_assert(mh->spanSize() == pageCount * kPageSize);
        //   // d_assert(mh->objectSize() == pageCount * kPageSize);

        unsafe { (*miniheap).malloc_at(0) }
    }

    fn free_for(&mut self, heap: *mut MiniHeap, offset: usize, mut epoch: Epoch) {
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
            let ptr = unsafe { self.arena.lock().unwrap().arena_begin.add(offset) as *mut () };

            let mh = {
                let arena = self.arena.lock().unwrap();
                let offset = unsafe {
                    usize::try_from(ptr.offset_from(arena.arena_begin as *mut ())).unwrap()
                };
                unsafe {
                    if let MiniHeapId::HeapPointer(ptr) = arena.mh_allocator.index(offset).unwrap()
                    {
                        ptr.as_mut().unwrap()
                    } else {
                        unreachable!()
                    }
                }
            };

            if epoch.0.load(Ordering::AcqRel) % 2 == 1 || !self.mesh_epoch.is_same(&epoch) {
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

    pub fn mini_heap_for_with_epoch(&self, ptr: *mut (), epoch: &mut Epoch) -> *mut MiniHeap {
        epoch.set(self.mesh_epoch.current(), Ordering::Acquire);

        let arena = self.arena.lock().unwrap();
        let offset = unsafe { ptr.offset_from(arena.arena_begin as *mut ()) } as usize;
        if let Some(MiniHeapId::HeapPointer(ptr)) = arena.mh_allocator.index(offset) {
            *ptr
        } else {
            unreachable!()
        }
    }

    #[allow(clippy::needless_for_each)]
    pub fn free_mini_heap_locked(&mut self, mini_heap: *mut (), untrack: bool) {
        let addr = self.arena.lock().unwrap().arena_begin;

        let mut to_free: [MaybeUninit<*mut MiniHeap>; MAX_MESHES] = MaybeUninit::uninit_array();

        let mh = unsafe { mini_heap.cast::<MiniHeap>().as_mut().unwrap() };
        let mut last = 0;

        crate::for_each_meshed!(mh {
            to_free[last].write(mh);
            last += 1;
            false
        });

        let to_free = unsafe { MaybeUninit::array_assume_init(to_free) };

        let begin = self.arena.lock().unwrap().arena_begin;

        to_free.iter().for_each(|heap| {
            let mh = unsafe { heap.as_mut().unwrap() };
            let mh_type = if mh.is_meshed() {
                PageType::Meshed
            } else {
                PageType::Dirty
            };
            let span_start = mh.span_start;
            unsafe { self.free(span_start as *mut ()) };

            if untrack && !mh.is_meshed() {
                self.untrack_mini_heap_locked(mini_heap);
            }

            unsafe { self.arena.lock().unwrap().mh_allocator.free(mini_heap) };

            self.mini_heap_count.fetch_sub(1, Ordering::AcqRel);
        });
    }

    pub fn untrack_mini_heap_locked(&self, mut mh: *mut ()) {
        self.stats.alloc_count.fetch_add(1, Ordering::AcqRel);
        let freelist = &self.free_lists.0;

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

    pub fn free(&mut self, ptr: *mut ()) {
        let mut start_epoch = Epoch::default();
        let offset = unsafe { ptr.offset_from(self.arena.lock().unwrap().arena_begin as *mut ()) };

        self.free_for(
            ptr as *mut MiniHeap,
            usize::try_from(offset).unwrap(),
            start_epoch,
        );
    }

    pub fn maybe_mesh(&mut self) {
        if self.access_lock.try_lock().is_ok() {
            self.mesh_all_sizes_mesh_locked();
        }
    }

    pub fn mesh_all_sizes_mesh_locked(&mut self) {
        // TODO:: add assert checks if needed

        self.arena.lock().unwrap().scavenge(true);

        if self.last_mesh_effective.load(Ordering::Acquire)
            && self.arena.lock().unwrap().above_mesh_threshold()
        {
            self.flush_all_bins();

            let total_mesh_count = (0..NUM_BINS)
                .map(|sz| self.mesh_size_class_locked(sz))
                .sum();

            unsafe {
                let merge_sets = &mut self.runtime.0.lock().unwrap().merge_set;
                merge_sets.madvise()
            };

            self.last_mesh_effective
                .store(total_mesh_count > 256, Ordering::Release);
            self.stats
                .mesh_count
                .fetch_add(total_mesh_count, Ordering::Acquire);

            self.arena.lock().unwrap().scavenge(true);
        }
    }

    pub fn small_alloc_mini_heaps<const N: usize>(
        &mut self,
        size_class: usize,
        object_size: usize,
        shuffle_vector: &[ShuffleVector<N>],
        current: u64,
    ) {
        let vectors: Vec<_> = shuffle_vector.iter().flat_map(|v| &v.mini_heaps).collect();

        vectors
            .iter()
            .for_each(|mut mh| self.release_mini_heap_locked(**mh));

        assert!(size_class < NUM_BINS);

        let (mut mini_heaps, mut bytes_free) = self.select_for_reuse(size_class, &vectors, current);

        if bytes_free < MINI_HEAP_REFILL_GOAL_SIZE && !mini_heaps.len() == mini_heaps.capacity() {
            let object_count = MIN_STRING_LEN.max(PAGE_SIZE / object_size);
            let page_count = page_count(object_size * object_count);

            while bytes_free < MINI_HEAP_REFILL_GOAL_SIZE
                && !mini_heaps.len() == mini_heaps.capacity()
            {
                let mut mh = self.alloc_mini_heap_locked(page_count, object_count, object_size, 1);
                let mut heap = unsafe { mh.as_mut().unwrap() };
                assert!(heap.is_attached());

                heap.set_attached(current, &heap.free_list as *const _ as *mut ListEntry);
                assert!(heap.is_attached() && heap.current() == current);
                mini_heaps.push(mh);
                bytes_free += heap.bytes_free();
            }
        }
    }

    pub fn release_mini_heap_locked(&mut self, mini_heap: *mut MiniHeap) {
        let mini_heap = unsafe { mini_heap.as_mut().unwrap() };
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
        let mut next = &self.free_lists.0[0][size_class].0.next;
        let mut to_be_locked = vec![];

        while let MiniHeapId::HeapPointer(next_id) = next {
            let mut mh = unsafe { next_id.as_mut().unwrap() };
            next = unsafe { &mh.free_list.next };

            to_be_locked.push(*next_id);
            self.free_lists.0[0][size_class]
                .1
                .store(1, Ordering::Acquire);
        }

        unsafe {
            assert_matches!(self.free_lists.0[0][size_class].0.next, MiniHeapId::Head);

            assert_matches!(self.free_lists.0[0][size_class].0.prev, MiniHeapId::Head);
        }

        to_be_locked.iter().for_each(|mh| {
            self.free_mini_heap_locked(mh as *const _ as *mut (), true);
        });
    }

    pub fn mesh_size_class_locked(&mut self, size_class: usize) -> usize {
        let merge_set_count = self.shifted_splitting(size_class);

        let mut merge_set = {
            let mut runtime = self.runtime.0.lock().unwrap();
            runtime.merge_set.merge_set
        };

        let mesh_count = merge_set.iter_mut().try_fold(0, |mut mesh_count, opt| {
            if let Some((mut dst, mut src)) = opt {
                let (src_obj, dst_obj) = unsafe { (src.as_mut().unwrap(), dst.as_mut().unwrap()) };

                let dst_count = dst_obj.mesh_count();
                let src_count = src_obj.mesh_count();
                if dst_count + src_count <= MAX_MESHES {
                    if dst_count < src_count {
                        std::mem::swap(&mut dst, &mut src);
                    }

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
                            self.mesh_locked(dst, src);
                            mesh_count += 1;
                        }
                    }
                }
                Some(mesh_count)
            } else {
                None
            }
        });

        self.flush_bin_locked(size_class);
        mesh_count.unwrap()
    }

    pub fn select_for_reuse(
        &self,
        size_class: usize,
        mini_heaps: &[&*mut MiniHeap],
        current: u64,
    ) -> (Vec<*mut MiniHeap>, usize) {
        let (mut mini_heaps, mut bytes_free) =
            self.fill_from_list(current, &self.free_lists.0[2][size_class]);
        if bytes_free < MINI_HEAP_REFILL_GOAL_SIZE {
            let (mh, bytes) = self.fill_from_list(current, &self.free_lists.0[0][size_class]);
            mini_heaps.extend_from_slice(&mh);
            bytes_free += bytes;
        }
        (mini_heaps, bytes_free)
    }

    pub fn fill_from_list(
        &self,
        current: u64,
        free_list: &(ListEntry, Comparatomic<AtomicU64>),
    ) -> (Vec<*mut MiniHeap>, usize) {
        let mut next = &free_list.0.next;
        let mut next_id = if let MiniHeapId::HeapPointer(p) = next {
            p
        } else {
            unreachable!()
        };
        let mut bytes_free = 0;

        let mut mini_heaps = vec![];
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
            let freelist = &self.free_lists;
            let size_class = heap.size_class() as usize;
            let fl = match &heap.free_list_id() {
                FreeListId::Empty => Some(&freelist.0[0][size_class].0),
                FreeListId::Full => Some(&freelist.0[1][size_class].0),
                FreeListId::Partial => Some(&freelist.0[2][size_class].0),
                _ => None,
            }
            .unwrap();

            heap.set_attached(current, fl as *const _ as *mut ListEntry);
            mini_heaps.push(mh as *const _ as *mut MiniHeap);
            count += 1;
            free_list.1.inner().fetch_sub(1, Ordering::AcqRel);
        }

        (mini_heaps, bytes_free)
    }

    pub fn shifted_splitting(&self, size_class: usize) -> Option<usize> {
        let free_lists = &self.free_lists.0;
        let runtime = &self.runtime.0.lock().unwrap();
        let left_set = runtime.merge_set.left;
        let right_set = runtime.merge_set.right;
        let splits = &runtime.merge_set;
        let mh = &free_lists[0].get(size_class).unwrap().0;
        let (left, right) = self.half_split(size_class);
        if left > 0 && right > 0 {
            let l = unsafe { splits.left.first().unwrap().unwrap().as_mut().unwrap() };

            assert_eq!((*l).bitmap().borrow_mut().byte_count(), 32usize);

            let merge_set_count = 0;
            Some((0..left).fold(0, move |mut count, j| {
                let mut idx_right = j;
                count += (0..right.min(64))
                    .scan((0, 0), move |(mut count, mut found_count), i| {
                        let bitmap1 = unsafe {
                            &left_set.get(j).unwrap().unwrap().as_mut().unwrap().bitmap()
                        };
                        let bitmap2 = unsafe {
                            &right_set
                                .get(j)
                                .unwrap()
                                .unwrap()
                                .as_mut()
                                .unwrap()
                                .bitmap()
                        };

                        let is_meshable = bitmap1
                            .borrow()
                            .internal_type
                            .bits()
                            .iter()
                            .zip(bitmap2.borrow().internal_type.bits().iter())
                            .fold(0u64, |mut acc, (lb, rb)| {
                                acc |= lb & rb;
                                acc
                            });

                        if is_meshable == 0 {
                            found_count += 1;

                            // left_set[j] = None;
                            // right_set[idx_right] = None;
                            idx_right += 1;
                            let merge_set_count =
                                self.mesh_found(&left_set[..], &right_set[..], merge_set_count);

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
        } else {
            None
        }
    }
    pub fn half_split(&self, size_class: usize) -> (usize, usize) {
        let lists = &self.free_lists.0;
        let mut next = &lists[2][size_class].0.next;

        let mut left_size = 0usize;
        let mut right_size = 0usize;

        while let MiniHeapId::HeapPointer(mh_id) = next && left_size < MAX_SPLIT_LIST_SIZE && right_size < MAX_SPLIT_LIST_SIZE
        {
            let mh = unsafe {
                mh_id.as_mut().unwrap()
            };
            next = &mh.free_list.next;
            if mh.is_meshing_candidate() || mh.fullness() >= OCCUPANCY_CUTOFF {
                let mut runtime = self.runtime.0.lock().unwrap();
                if left_size <= right_size {
                    runtime.merge_set.left[left_size] = Some(mh as *const _ as *mut MiniHeap);
                    left_size += 1;
                } else {
                    runtime.merge_set.right[right_size] = Some(mh as *const _ as *mut MiniHeap);
                    right_size += 1;
                }

                let mut rng = &self.rng;
                self.rng.shuffle(&mut runtime.merge_set.left, 0, left_size);
                self.rng.shuffle(&mut runtime.merge_set.right, 0, right_size);
            }
        }

        (left_size, right_size)
    }

    pub fn mesh_found(
        &self,
        left: &[Option<*mut MiniHeap>],
        right: &[Option<*mut MiniHeap>],
        mut merge_set_count: usize,
    ) -> usize {
        let mut runtime = self.runtime.0.lock().unwrap();
        let merge_set_count = left.iter().zip(right.iter()).fold(merge_set_count, |mut acc, (l, r)| {
        if let Some(le) = l && unsafe { le.as_mut().unwrap().is_meshing_candidate() } && let Some(ri) = r &&  unsafe { ri.as_mut().unwrap().is_meshing_candidate() } {
           runtime.deref_mut().merge_set.merge_set[merge_set_count] = Some((*le, *ri));
           acc += 1;
        }
        acc
        });
        merge_set_count
    }

    pub fn post_free_locked(&mut self, mh: *mut MiniHeap, in_use: usize) -> Option<()> {
        let mini_heap = unsafe { mh.as_mut().unwrap() };
        mini_heap.is_attached().then_some(())?;
        let mut free_lists = &mut self.free_lists;
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
            .load(Ordering::Release);

        (empties > BINNED_TRACKER_MAX_EMPTY).then_some(())
    }

    pub fn mesh_locked(&self, dst: *mut MiniHeap, src: *mut MiniHeap) {
        let dst = unsafe { dst.as_mut().unwrap() };

        let src = unsafe { src.as_mut().unwrap() };

        crate::for_each_meshed!(src {
            let src_span = src.span_start as *mut libc::c_void;
            self.arena
                .lock()
                .unwrap()
                .begin_mesh(src_span, dst.span_size());
            false
        });

        dst.consume(src);
    }

    pub fn page_aligned_alloc(&mut self, alignment: usize, size: usize) -> *mut () {
        let page_count = page_count(size);
        let mh = unsafe {
            self.alloc_mini_heap_locked(page_count, 1, page_count * PAGE_SIZE, alignment)
                .as_ref()
                .unwrap()
        };

        assert!(mh.is_large_alloc());
        assert!(mh.span_size() == page_count * PAGE_SIZE);

        unsafe { mh.malloc_at(0) }
    }

    pub fn alloc_mini_heap_locked(
        &self,
        page_count: usize,
        object_count: usize,
        object_size: usize,
        alignment: usize,
    ) -> *mut MiniHeap {
        let mut arena = self.arena.lock().unwrap();
        let buffer = unsafe { arena.mh_allocator.alloc() };
        let span = Span::default();
        let span_begin = arena.page_alloc(page_count, alignment);
        let mut mh = MiniHeap::with_object(span.clone(), object_count, object_size);
        let mini_heap_id = unsafe { arena.mh_allocator.offset_for(buffer) };
        unsafe { arena.track_miniheap(&span, buffer.cast()) };

        self.mini_heap_count.fetch_add(1, Ordering::Acquire);
        self.stats.alloc_count.fetch_add(1, Ordering::AcqRel);
        let hwm = self.stats.high_water_mark.load(Ordering::Acquire);
        let count = self.mini_heap_count.load(Ordering::Acquire);
        if count > hwm {
            self.stats.high_water_mark.store(count, Ordering::Release);
        }
        &mut mh as *mut MiniHeap
    }

    fn alloc_miniheap(
        &mut self,
        page_count: usize,
        object_count: usize,
        object_size: usize,
        page_align: usize,
    ) -> *mut MiniHeap {
        debug_assert!(page_count > 0, "should allocate at least 1 page");

        let mut arena = self.arena.lock().unwrap();
        let buf = unsafe { arena.mh_allocator.alloc() };
        debug_assert_ne!(buf, null_mut());
        // allocate out of the arena
        let (span, span_begin) = arena.page_alloc(page_count, page_align);
        debug_assert_ne!(span_begin, null_mut(), "arena allocation failed");
        debug_assert_eq!(
            span_begin
                .cast::<[u8; PAGE_SIZE]>()
                .align_offset(page_align),
            0,
            "arena allocation unaligned"
        );
        debug_assert!(size_of::<MiniHeap>() <= 64);
        let mh = buf.cast();
        unsafe { MiniHeap::new_inplace(mh, span.clone(), object_count, object_size) }
        unsafe { arena.track_miniheap(&span, buf.cast()) };

        // // mesh::debug("%p (%u) created!\n", mh, GetMiniHeapID(mh));

        self.mini_heap_count.fetch_add(1, Ordering::AcqRel);
        self.stats.alloc_count.fetch_add(1, Ordering::AcqRel);
        let count = self.mini_heap_count.load(Ordering::Acquire);
        self.stats.high_water_mark.store(count, Ordering::Release);

        mh
    }
}

#[derive(Default, PartialEq)]
pub struct Epoch(Comparatomic<AtomicU64>);

impl Epoch {
    pub fn current(&self) -> u64 {
        self.0.inner().load(Ordering::SeqCst)
    }

    pub fn is_same(&self, start_epoch: &Epoch) -> bool {
        self == start_epoch
    }

    pub fn set(&self, value: u64, ordering: Ordering) {
        self.0.inner().store(value, ordering)
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
