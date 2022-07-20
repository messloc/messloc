use std::{thread::ThreadId, cell::UnsafeCell, alloc::Layout, ptr::{NonNull, slice_from_raw_parts_mut}};

use rand::{Rng, SeedableRng};
use spin::{Mutex, RwLock, RwLockUpgradableGuard};

use super::{span::SpanAllocator, SpanVec, thread_heap::ThreadHeap, SIZE_CLASS_COUNT, SIZE_CLASSES, shuffle_vec::ShuffleVector, mini_heap::MiniHeap};
use super::MAX_ALLOCATIONS_PER_SPAN;

type ThreadLocalLookup<R, S> = SpanVec<(usize, UnsafeCell<ThreadHeap<R, <S as SpanAllocator>::Handle>>), S>;

pub struct GlobalHeap<R, S: SpanAllocator> {
    rng: Mutex<R>,
    span_alloc: S,
    thread_local: RwLock<ThreadLocalLookup<R, S>>,
}

impl<R, S: SpanAllocator + Clone> GlobalHeap<R, S> {
    pub fn new(span_alloc: S, rng: R) -> Result<Self, S::AllocError> {
        let thread_local =
            unsafe { SpanVec::with_capacity_in(0, span_alloc.clone())? };

        Ok(GlobalHeap {
            rng: Mutex::new(rng),
            span_alloc,
            thread_local: RwLock::new(thread_local),
        })
    }
}

unsafe impl<R, S: SpanAllocator> Sync for GlobalHeap<R, S> {}

impl<R: Rng + SeedableRng, S: SpanAllocator> GlobalHeap<R, S> {
    pub unsafe fn alloc(&self, layout: Layout) -> Result<NonNull<[u8]>, S::AllocError> {
        // check if the size is over 16k
        if layout.size() > SIZE_CLASSES[SIZE_CLASS_COUNT - 1] as usize {
            // use large alloc strategy
            // todo!()
            let ptr = self.span_alloc.allocate_span(100)?.data_ptr().as_ptr();
            return unsafe { Ok(NonNull::new_unchecked(slice_from_raw_parts_mut(ptr, self.span_alloc.page_size() * 100))) };
        }

        // let thread_id = std::thread::current().id();
        let thread_id = ::thread_id::get();

        let thread_local = self.thread_local.upgradeable_read();
        let mut thread_heap = if let Some(item) = thread_local
            .as_slice()
            .iter()
            .find(|(item_id, _)| *item_id == thread_id)
        {
            item.1.get()
        } else {
            self.create_thread_heap(thread_id, thread_local)?
        };

        // try to alloc using thread heap
        // TODO:
        // Ok(NonNull::new(slice_from_raw_parts_mut(NonNull::dangling().as_ptr(), 0)).unwrap())
        let thread_heap = &mut *thread_heap;
        match thread_heap.alloc(layout) {
            Ok(ptr) => Ok(ptr),
            Err(request) => {
                let pages = ((request.size_class() as usize * MAX_ALLOCATIONS_PER_SPAN) / self.span_alloc.page_size()).min(u16::MAX as usize).max(1);
                let mini_heap = MiniHeap::new(self.span_alloc.allocate_span(pages.try_into().unwrap())?, self.span_alloc.page_size(), request.size_class());
                let old_mini_heap = thread_heap.replace_mini_heap(request, mini_heap);
                Ok(thread_heap.alloc(layout).map_err(|_| ()).expect("Mini heap was just replaced"))
            }
        }
    }

    fn create_thread_heap(&self, thread_id: usize, thread_local: RwLockUpgradableGuard<'_, ThreadLocalLookup<R, S>>) -> Result<*mut ThreadHeap<R, S::Handle>, S::AllocError> {
        let item = (
            thread_id,
            UnsafeCell::new(ThreadHeap::new(R::seed_from_u64({ let mut rng = self.rng.lock(); rng.next_u64() }))),
        );

        let mut thread_local = thread_local.upgrade();
        let index = match thread_local.push(item) {
            Ok(index) => index,
            Err(item) => {
                thread_local.reserve(1)?;
                thread_local.push(item).unwrap()
            }
        };
        let thread_local = thread_local.downgrade();

        let slice = thread_local.as_slice();
        Ok(slice[index].1.get())
    }
}

