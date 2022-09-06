use std::{
    mem::size_of,
    ops::DerefMut,
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard, PoisonError,
    },
    time::{SystemTime, Duration},
};

use arrayvec::ArrayVec;

use crate::{
    span::Span,
    comparatomic::Comparatomic,
    list_entry::ListEntry,
    meshable_arena::{MeshableArena, PageType},
    mini_heap::{self, AtomicMiniHeapId, FreeListId, MiniHeap, MiniHeapId},
    one_way_mmap_heap::Heap,
    runtime::{self, Runtime},
    BINNED_TRACKER_MAX_EMPTY, MAX_MERGE_SETS, MAX_MESHES, MAX_MESHES_PER_ITERATION,
    MAX_SPLIT_LIST_SIZE, NUM_BINS, OCCUPANCY_CUTOFF, PAGE_SIZE, cheap_heap::CheapHeap, MINI_HEAP_REFILL_GOAL_SIZE, MIN_STRING_LEN, 
};

pub struct GlobalHeapStats {
    mesh_count: AtomicUsize,
    free_count: usize,
    alloc_count: usize,
    high_water_mark: usize,
}

pub struct GlobalHeapShared;

pub struct GlobalHeapGuarded<'a> {
    pub arena: MeshableArena<'a>,
    miniheap_count: usize,
    stats: GlobalHeapStats,
}

pub struct GlobalHeap<'a> {
    runtime: Runtime<'a>,
    shared: GlobalHeapShared,
    pub guarded:Mutex<GlobalHeapGuarded<'a>>,
    mini_heap: MiniHeap<'a>,
    last_mesh_effective: AtomicBool,
    mesh_epoch: Epoch,
    miniheap_count: Comparatomic<AtomicU32>,
    access_lock: Mutex<()>,
    mesh_period_ms: Duration,
}
pub struct GlobalHeapLocked<'lock, 'a: 'lock> {
    shared: &'lock GlobalHeapShared,
    guarded: MutexGuard<'lock, GlobalHeapGuarded<'a>>,
}

/// Returns the minimum number of pages needed to
/// hold the requested allocation
const fn page_count(bytes: usize) -> usize {
    // bytes.div_ceil(PAGE_SIZE)
    (bytes.wrapping_add(PAGE_SIZE - 1)) / PAGE_SIZE
}

impl GlobalHeap<'_> {
    /// Lock access to the GlobalHeap
    pub fn lock(&self) -> GlobalHeapLocked<'_, '_> {
        let guarded = self.guarded.lock().unwrap_or_else(PoisonError::into_inner);
        GlobalHeapLocked {
            shared: &self.shared,
            guarded,
        }
    }

    /// Allocate a region of memory that can satisfy the requested bytes
    pub fn malloc(&self, bytes: usize) -> *const () {
        self.alloc_page_aligned(1, page_count(bytes))
    }

    /// Allocate the requested number of pages
    fn alloc_page_aligned(&self, page_align: usize, page_count: usize) -> *const () {
        // if given a very large allocation size (e.g. (usize::MAX)-8), it is possible
        // the pages calculation overflowed. An allocation that big is impossible
        // to satisfy anyway, so just fail early.
        if page_count == 0 {
            return null();
        }

        let mut lock = self.lock();

        let miniheap = lock.alloc_miniheap(page_count, 1, page_count * PAGE_SIZE, page_align);

        //   d_assert(mh->isLargeAlloc());
        //   d_assert(mh->spanSize() == pageCount * kPageSize);
        //   // d_assert(mh->objectSize() == pageCount * kPageSize);

        unsafe { (*miniheap).malloc_at(lock.guarded.arena.arena_begin, 0) }
    }

    fn free_for(&self, mini_heap: &mut MiniHeap<'_>, offset: usize, start_epoch: Epoch) {
        if mini_heap.is_large_alloc() {
            self.mini_heap.lock();
            self.free_mini_heap_locked(&mut mini_heap, false);
        } else {
            assert!(mini_heap.max_count() > 1);

            self.last_mesh_effective.compare_exchange(
                false,
                true,
                Ordering::AcqRel,
                Ordering::Release,
            );
            let remaining = mini_heap.in_use_count() - 1;
            let was_set = mini_heap.clear_if_not_free(offset);
            let ptr = unsafe { self.guarded.lock().unwrap().arena.arena_begin.add(offset) };

            if start_epoch.0.load(Ordering::AcqRel) % 2 == 1
                || !self.mesh_epoch.is_same(start_epoch)
            {
                let mh = unsafe {
                    std::ptr::read(self.mini_heap_for_with_epoch(ptr as *mut (), &mut start_epoch))
                };
                if mh.is_related(mini_heap) && was_set {
                    assert_eq!(mini_heap.size_class(), mh.size_class());
                    unsafe { mh.free(ptr as *mut ()) };

                    if mh.is_attached()
                        && (mh.in_use_count() == 0 || mh.free_list_id() == FreeListId::Full)
                    {
                        if self.post_free_locked(&mh, 0).is_some() {
                            self.flush_bin_locked(mh.size_class() as usize);
                        }
                    } else {
                        self.maybe_mesh();
                    }
                }
            } else {
                if mini_heap.is_attached()
                    && (remaining == 0 || mini_heap.free_list_id() == FreeListId::Full)
                {
                    self.mini_heap.lock();
                    let mh = unsafe {
                        self.mini_heap_for_with_epoch(ptr as *mut (), &mut start_epoch)
                            .as_ref()
                            .unwrap()
                    };
                    if mh != mini_heap {
                        if mh.is_related(mini_heap) {
                            // TODO: store created_epoch on mh and check it here
                        }
                    } else {
                        if self.post_free_locked(mh, mh.in_use_count()).is_some() {
                            self.flush_bin_locked(mh.size_class() as usize);
                        }
                    }
                } else {
                    self.maybe_mesh();
                }
            }
        }
    }

    pub fn mini_heap_for_with_epoch(&self, ptr: *mut (), epoch: &mut Epoch) -> *mut MiniHeap<'_> {
        epoch.set(self.mesh_epoch.current(), Ordering::Acquire);
        self.mini_heap_for(ptr)
    }

    pub fn mini_heap_for(&self, ptr: *mut ()) -> *mut MiniHeap<'_> {
        self.guarded.lock().unwrap().arena.lookup_mini_heap(ptr)
    }

    pub fn mini_heap_for_id(&self, id: AtomicMiniHeapId<MiniHeap<'_>>) -> *mut MiniHeap<'_> {
        self.guarded.lock().unwrap().arena.mini_heap_for_id(id)
    }

    pub fn free_mini_heap_locked(&mut self, mh: &mut MiniHeap<'_>, untrack: bool) {
        let to_free = [MiniHeap::new(); MAX_MESHES];
        let mut last = 0;

        mh.for_each_meshed(|mh| {
            to_free[last] = mh;
            false
        });

        let begin = self.guarded.lock().unwrap().arena.arena_begin;

        to_free.iter().for_each(|heap| {
            let mh = *heap;
            let mh_type = if mh.is_meshed() {
                PageType::Meshed
            } else {
                PageType::Dirty
            };
            unsafe { self.free(mh.get_span_start() as *mut ())};
            self.guarded.lock().unwrap().stats.free_count += 1;
            self.free_mini_heap_after_mesh_locked(&mut mh, untrack);
        });
    }

    pub fn free_mini_heap_after_mesh_locked(&mut self, mh: &mut MiniHeap<'_>, untrack: bool) {
        if untrack && !mh.is_meshed() {
            self.untrack_mini_heap_locked(&mh);
        }

        unsafe {
            self.guarded
                .lock()
                .unwrap()
                .arena
                .mh_allocator
                .free(mh as *mut MiniHeap<'_> as *mut ());
        };

        self.miniheap_count.inner().fetch_sub(1, Ordering::AcqRel);
    }

    pub fn untrack_mini_heap_locked(&mut self, mh: &MiniHeap<'_>) {
        self.guarded.lock().unwrap().stats.alloc_count -= 1;
        mh.get_free_list().remove(self.free_list_for(mh).unwrap());
    }

    pub fn free_list_for(&self, mh: &MiniHeap<'_>) -> Option<ListEntry<'_>> {
        let freelist = self.runtime.lock().unwrap().free_lists;
        let size_class = mh.size_class() as usize;
        match mh.free_list_id() {
            FreeListId::Empty => Some(freelist[0][size_class].0),
            FreeListId::Full => Some(freelist[1][size_class].0),
            FreeListId::Partial => Some(freelist[2][size_class].0),
            _ => None,
        }
    }

    pub fn free(&mut self, ptr: *mut ()) {
        let mut start_epoch = Epoch::default();
        let mh = unsafe {
            self.mini_heap_for_with_epoch(ptr, &mut start_epoch)
                .as_ref()
                .unwrap()
        };
        let offset =
            unsafe { ptr.offset_from(self.guarded.lock().unwrap().arena.arena_begin as *mut ()) };

        self.free_for(&mut mh, usize::try_from(offset).unwrap(), start_epoch);
    }

    pub fn maybe_mesh(&self) {
        if self.access_lock.try_lock().is_ok() {
            self.mesh_all_sizes_mesh_locked();
        }
    }

    pub fn mesh_all_sizes_mesh_locked(&self) {
        let merge_sets = (*self.runtime.lock().unwrap()).merge_set;
        // TODO:: add assert checks if needed

        self.guarded.lock().unwrap().arena.scavenge(true);

        if self.last_mesh_effective.load(Ordering::Acquire)
            && self.guarded.lock().unwrap().arena.above_mesh_threshold()
        {
            let total_mesh_count = (0..NUM_BINS)
                .map(|sz| {
                    self.flush_bin_locked(sz);
                    sz
                })
                .map(|sz| self.mesh_size_class_locked(sz))
                .sum();

            unsafe { merge_sets.madvise() };

            self.last_mesh_effective
                .store(total_mesh_count > 256, Ordering::Acquire);
            self.guarded
                .lock()
                .unwrap()
                .stats
                .mesh_count
                .fetch_add(total_mesh_count, Ordering::Acquire);

            self.guarded.lock().unwrap().arena.scavenge(true);
        }
    }

        pub fn small_alloc_mini_heaps<'a, const N: usize>(&'a self, size_class: usize, object_size: usize, mini_heaps: &mut ArrayVec<&'a MiniHeap<'a>, N>, current: u64) {
            mini_heaps.iter_mut().for_each(|mh| self.release_mini_heap_locked(mh));

            mini_heaps.clear();

            assert!(size_class < NUM_BINS);

            let bytes_free = self.select_for_reuse(size_class, mini_heaps, current);

            if bytes_free < MINI_HEAP_REFILL_GOAL_SIZE && !mini_heaps.is_full() {
                let object_count = MIN_STRING_LEN.max(PAGE_SIZE / object_size);
                let page_count = page_count(object_size * object_count);

                while bytes_free < MINI_HEAP_REFILL_GOAL_SIZE && !mini_heaps.is_full() {
                let mh = unsafe { self.alloc_mini_heap_locked(page_count, object_count, object_size, 1).as_ref().unwrap() };
                assert!(mh.is_attached());
                mh.set_attached(current, mh.get_free_list());
                assert!(mh.is_attached() && mh.current() == current);
                mini_heaps.push(mh);
                bytes_free += mh.bytes_free();
                }
            }


        }


        pub fn release_mini_heap_locked(&mut self, mini_heap: &mut MiniHeap<'_>) {
            mini_heap.unset_attached();
            self.post_free_locked(mini_heap, mini_heap.in_use_count());
        }

        pub fn set_mesh_period_ms(&self, period: Duration) {
            self.mesh_period_ms = period;
        }


    pub fn flush_bin_locked(&self, size_class: usize) {
        let mut empty = (*self.runtime.lock().unwrap()).free_lists[0];
        let mut next = empty[size_class].0.next.unwrap();

        if !next.is_head() {
            while (!next.is_head()) {
                let mut mh = unsafe {
                    self.runtime
                        .lock()
                        .unwrap()
                        .global_heap
                        .mini_heap_for_id(next)
                        .as_mut()
                        .unwrap()
                };
                next = mh.get_free_list().next.unwrap();
                self.free_mini_heap_locked(mh, true);
                empty[size_class].1 -= 1;
            }

            assert!(empty[size_class].0.next.unwrap().is_head());
            assert!(empty[size_class].0.prev.unwrap().is_head());
        }
    }

    pub fn mesh_size_class_locked(&self, size_class: usize) -> usize {
        let merge_set_count = self.shifted_splitting(size_class);

        let mesh_count = (*self.runtime.lock().unwrap())
            .merge_set
            .merge_set
            .iter_mut()
            .fold(0, |mut mesh_count, (mut dst, mut src)| {
                let dst_count = dst.mesh_count();
                let src_count = src.mesh_count();
                if dst_count + src_count <= MAX_MESHES {
                    if dst_count < src_count {
                        std::mem::swap(&mut dst, &mut src);
                    }

                    match (dst.in_use_count(), src.in_use_count()) {
                        (0, 0) => {
                            self.post_free_locked(&dst, 0).unwrap();
                            self.post_free_locked(&src, 0).unwrap();
                        }
                        (0, _) => {
                            self.post_free_locked(&dst, 0).unwrap();
                        }

                        (_, 0) => {
                            self.post_free_locked(&src, 0);
                        }
                        _ => {
                            self.mesh_locked(dst, src);
                            mesh_count += 1;
                        }
                    }
                }
                mesh_count
            });

        self.flush_bin_locked(size_class);
        mesh_count
    }

    pub fn select_for_reuse<'a, const N: usize>(&mut self, size_class: usize, mini_heaps: &mut ArrayVec<&'a MiniHeap<'a>, N>, current: u64) -> usize {

        let lists = self.runtime.lock().unwrap().free_lists;
            let bytes_free = self.fill_from_list(mini_heaps, current, lists[2][size_class]);
        if bytes_free >= MINI_HEAP_REFILL_GOAL_SIZE || mini_heaps.is_full() {
            bytes_free
        } else {
            bytes_free += self.fill_from_list(mini_heaps, current, lists[0][size_class]);
            bytes_free
        }
    }

    pub fn fill_from_list<'a, const N: usize>(&mut self, mini_heaps: &ArrayVec<&'a MiniHeap<'a>, N>, current: u64, free_list: (ListEntry<'a>, u64)) -> usize {

        let mut next_id = free_list.0.next.unwrap();
        let bytes_free = 0;
        while !next_id.is_head() && bytes_free < MINI_HEAP_REFILL_GOAL_SIZE && !mini_heaps.is_full() {
            let mh = unsafe { self.mini_heap_for_id(next_id).as_ref().unwrap() };
            next_id = mh.get_free_list().next.unwrap();
            //TODO: remove if removing it causes better experience as discovered in the original
            //source 
            bytes_free += mh.bytes_free();
            assert!(mh.is_attached());
            mh.set_attached(current, &self.free_list_for(mh).unwrap());
            mini_heaps.push(mh);
            free_list.1.saturating_sub(1);
            } 

        bytes_free
        
    }


    pub fn shifted_splitting(&self, size_class: usize) -> usize {
        let free_lists = self.runtime.lock().unwrap().free_lists;
        let mh = free_lists[0].get(size_class).unwrap().0;
        let (left, right) = self.half_split(size_class);
        if left > 0 && right > 0 {
            assert!(free_lists.left.first().unwrap().bitmap().byte_count() == 32);
            let mut merge_sets = self.runtime.lock().unwrap().deref_mut().merge_set;

            let left_set = merge_sets.left;
            let right_set = merge_sets.right;
            let merge_set_count = 0;
            (0..left).fold(0, |mut count, j| {
                let mut idx_right = j;
                count += (0..right.min(64))
                    .scan((0, 0), |(mut count, mut found_count), i| {
                        let bitmap1 = left_set.get(j).unwrap().unwrap().bitmap().bits();
                        let bitmap2 = right_set.get(j).unwrap().unwrap().bitmap().bits();
                        if bitmap1.is_meshable(bitmap2) {
                            found_count += 1;

                            left_set[j] = None;
                            right_set[idx_right] = None;
                            idx_right += 1;
                            let left = merge_sets.left;
                            let right = merge_sets.right;

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
            })
        }
    }

    pub fn half_split(&self, size_class: usize) -> (usize, usize) {
        let mut mh_id = self
            .runtime
            .lock()
            .unwrap()
            .free_lists
            .partial
            .get(size_class)
            .unwrap()
            .0
            .next();

        let left_size = 0usize;
        let right_size = 0usize;
        while mh_id != List::Head
            && left_size < MAX_SPLIT_LIST_SIZE
            && right_size < MAX_SPLIT_LIST_SIZE
        {
            let mh = unsafe {
                (*self.runtime.lock().unwrap())
                    .global_heap
                    .mini_heap_for_id(mh_id)
                    .as_ref()
                    .unwrap()
            };
            mh_id = mh.get_free_list().next.unwrap();

            if mh.is_meshing_candidate() || mh.fullness() >= OCCUPANCY_CUTOFF {
                let mut merge_set = self.runtime.lock().unwrap().merge_set;
                if left_size <= right_size {
                    merge_set.left[left_size] = Some(mh);
                    left_size += 1;
                } else {
                    merge_set.right[right_size] = Some(mh);
                    right_size += 1;
                }

                let merge_set = self.runtime.lock().unwrap().merge_set;
                let rng = self.runtime.lock().unwrap().rng;
                rng.shuffle(&mut merge_set.left, 0, left_size);
                rng.shuffle(&mut merge_set.right, 0, right_size);
            }
        }

        (left_size, right_size)
    }

    pub fn mesh_found(
        &self,
        left: &[Option<&MiniHeap<'_>>],
        right: &[Option<&MiniHeap<'_>>],
        mut merge_set_count: usize,
    ) -> usize {
        let merge_sets = self.runtime.lock().unwrap().merge_set;
        let merge_set_count = left.iter().zip(right.iter()).fold(merge_set_count, |mut acc, (l, r)| {
        if let Some(le) = l && le.is_meshing_candidate() && let Some(ri) = r &&  ri.is_meshing_candidate() {
           merge_sets.merge_set[merge_set_count] = (le, ri);
           acc += 1;
        }
        acc
        });
        merge_set_count
    }

    pub fn post_free_locked(&self, mini_heap: &MiniHeap<'_>, in_use: usize) -> Option<()> {
        let _ = mini_heap.is_attached().then_some(())?;
        let free_lists = self.runtime.lock().unwrap().free_lists;
        let current_free_list = self.free_list_for(&mini_heap);
        let free_list_id = mini_heap.free_list_id();
        let max_count = usize::try_from(mini_heap.max_count()).unwrap();
        let size_class = mini_heap.size_class() as usize;

        let (new_list_id, list) = match (in_use, free_list_id) {
            (0, FreeListId::Empty) => return None,
            (iu, FreeListId::Full) if iu == max_count => return None,
            (0, _) => (FreeListId::Empty, free_lists[0].get(size_class).unwrap()),
            (iu, _) if iu == max_count => {
                (FreeListId::Full, free_lists[1].get(size_class).unwrap())
            }
            (_, FreeListId::Partial) => return None,
            _ => (FreeListId::Partial, free_lists[2].get(size_class).unwrap()),
        };
        list.0.add(
            current_free_list.unwrap(),
            new_list_id as u32,
            AtomicMiniHeapId::new(null_mut()),
            &mut mini_heap,
        );
        list.1 += 1;

        (free_lists[0].get(size_class).unwrap().1 > BINNED_TRACKER_MAX_EMPTY).then_some(())
    }

    pub fn mesh_locked(&self, dst: &MiniHeap<'_>, src: &MiniHeap<'_>) {
        src.for_each_meshed(|mh| {
            let src_span = unsafe { mh.get_span_start()};
            self.guarded
                .lock()
                .unwrap()
                .arena
                .begin_mesh(src_span, dst.span_size());
            false
        });

        dst.consume(src);
    }

    pub fn page_aligned_alloc(&self, alignment: usize, size: usize) -> *mut () {
        let page_count = page_count(size);
        let mh = unsafe { self.alloc_mini_heap_locked(page_count, 1, page_count * PAGE_SIZE, alignment).as_ref().unwrap() };

        assert!(mh.is_large_alloc());
        assert!(mh.span_size() == page_count * PAGE_SIZE);

        unsafe { mh.malloc_at(self.guarded.lock().unwrap().arena.arena_begin, 0) }
    }


    pub fn alloc_mini_heap_locked(&self, page_count: usize, object_count: usize, object_size: usize, alignment: usize) -> *mut MiniHeap<'_> {

        let buffer = unsafe { (*self.guarded.lock().unwrap()).arena.mh_allocator.alloc() };
        let span = Span::default();
        let span_begin = self.guarded.lock().unwrap().arena.page_alloc(page_count, alignment);
        let mh = MiniHeap::with_object(span, object_count, object_size);
        let mini_heap_id = unsafe { (*self.guarded.lock().unwrap()).arena.mh_allocator.offset_for(buffer) };
        unsafe { (*self.guarded.lock().unwrap()).arena.track_miniheap(span, buffer.cast()) };

        let stats = (*self.guarded.lock().unwrap()).stats;
        self.miniheap_count.inner().fetch_add(1, Ordering::Acquire);
        stats.alloc_count += 1;
        stats.high_water_mark = stats.high_water_mark.max(self.miniheap_count.load(Ordering::AcqRel) as usize);
        &mut mh as *mut MiniHeap<'_>

    }

    pub fn dump_strings(self) {
        //TODO: this isnt implemented in the source and its not a major fuctionality blocker so
        //implement it by implementing print occupancy.

        todo!()
    }
}

impl GlobalHeapLocked<'_, '_> {
    fn alloc_miniheap(
        &mut self,
        page_count: usize,
        object_count: usize,
        object_size: usize,
        page_align: usize,
    ) -> *mut MiniHeap<'_> {
        debug_assert!(page_count > 0, "should allocate at least 1 page");

        let buf = unsafe { self.guarded.arena.mh_allocator.alloc() };
        debug_assert_ne!(buf, null_mut());

        // allocate out of the arena
        let (span, span_begin) = self.guarded.arena.page_alloc(page_count, page_align);

        debug_assert_ne!(span_begin, null_mut(), "arena allocation failed");
        debug_assert_eq!(
            span_begin
                .cast::<[u8; PAGE_SIZE]>()
                .align_offset(page_align),
            0,
            "arena allocation unaligned"
        );

        debug_assert!(size_of::<MiniHeap<'_>>() <= 64);
        let mh = buf.cast();
        unsafe { MiniHeap::new_inplace(mh, span.clone(), object_count, object_size) }

        unsafe { self.guarded.arena.track_miniheap(span, buf.cast()) };

        // // mesh::debug("%p (%u) created!\n", mh, GetMiniHeapID(mh));

        self.guarded.miniheap_count += 1;
        self.guarded.stats.alloc_count += 1;
        self.guarded.stats.high_water_mark = self
            .guarded
            .miniheap_count
            .max(self.guarded.stats.high_water_mark);

        mh
    }
}

#[derive(Default, PartialEq)]
struct Epoch(Comparatomic<AtomicU64>);

impl Epoch {
    pub fn current(&self) -> u64 {
        self.0.inner().load(Ordering::SeqCst)
    }

    pub fn is_same(&self, start_epoch: Epoch) -> bool {
        *self == start_epoch
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
