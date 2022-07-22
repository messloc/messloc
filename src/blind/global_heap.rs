use std::{thread::ThreadId, cell::UnsafeCell, alloc::Layout, ptr::{NonNull, slice_from_raw_parts_mut}};

use rand::{Rng, SeedableRng};
use spin::{Mutex, RwLock, RwLockUpgradableGuard};

use super::{span::{SpanAllocator, Span}, SpanVec, thread_heap::ThreadHeap, SIZE_CLASS_COUNT, SIZE_CLASSES, shuffle_vec::ShuffleVector, mini_heap::MiniHeap, div_ceil};
use super::MAX_ALLOCATIONS_PER_SPAN;

type ThreadLocalLookup<R, S> = SpanVec<(usize, UnsafeCell<ThreadHeap<R, <S as SpanAllocator>::Handle>>), S>;

pub struct GlobalHeap<R, S: SpanAllocator> {
    rng: Mutex<R>,
    span_alloc: S,
    thread_local: RwLock<ThreadLocalLookup<R, S>>,
    extra_mini_heaps: [Mutex<SpanVec<MiniHeap<S::Handle>, S>>; SIZE_CLASS_COUNT],
    large_spans: Mutex<SpanVec<Span<S::Handle>, S>>,
}

impl<R, S: SpanAllocator> Drop for GlobalHeap<R, S> {
    fn drop(&mut self) {
        for thread_local in self.thread_local.get_mut().as_slice_mut() {
            unsafe { (&mut *thread_local.1.get_mut()).drop_heaps(&mut self.span_alloc) }.map_err(|_| ()).unwrap();
        }

        for mini_heaps in &mut self.extra_mini_heaps {
            for mini_heap in mini_heaps.get_mut().as_slice_mut() {
                unsafe { self.span_alloc.deallocate_span(mini_heap.span_mut()).map_err(|_| ()).unwrap(); }
            }
        }

        for large_span in self.large_spans.get_mut().as_slice_mut() {
            unsafe { self.span_alloc.deallocate_span(large_span).map_err(|_| ()).unwrap()}
        }
    }
}

impl<R, S: SpanAllocator + Clone> GlobalHeap<R, S> {
    pub fn new(span_alloc: S, rng: R) -> Result<Self, S::AllocError> {
        let thread_local =
            unsafe { SpanVec::with_capacity_in(0, span_alloc.clone())? };

        let extra_mini_heaps = array_init::try_array_init(|_| Ok(Mutex::new(SpanVec::with_capacity_in(0, span_alloc.clone())?)))?;

        let large_spans = Mutex::new(SpanVec::with_capacity_in(0, span_alloc.clone())?);

        Ok(GlobalHeap {
            rng: Mutex::new(rng),
            span_alloc,
            thread_local: RwLock::new(thread_local),
            extra_mini_heaps,
            large_spans,
        })
    }
}

unsafe impl<R, S: SpanAllocator + Sync> Sync for GlobalHeap<R, S> {}
unsafe impl<R, S: SpanAllocator + Send> Send for GlobalHeap<R, S> {}

impl<R: Rng + SeedableRng, S: SpanAllocator> GlobalHeap<R, S> {
    pub unsafe fn alloc(&self, layout: Layout) -> Result<NonNull<[u8]>, S::AllocError> {
        // check if the size is over 16k
        if layout.size() > SIZE_CLASSES[SIZE_CLASS_COUNT - 1] as usize {
            // use large alloc strategy
            let pages = div_ceil(layout.size(), self.span_alloc.page_size());
            let span = self.span_alloc.allocate_span(pages)?;
            let ptr = unsafe { NonNull::new_unchecked(slice_from_raw_parts_mut(span.data_ptr().as_ptr(), span.pages() as usize * self.span_alloc.page_size())) };
            let mut large_spans = self.large_spans.lock();
            large_spans.push(span);
            drop(large_spans);
            return Ok(ptr);
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
        let thread_heap = &mut *thread_heap;
        match thread_heap.alloc(layout) {
            Ok(ptr) => Ok(ptr),
            Err(request) => {
                let pages = ((request.size_class() as usize * MAX_ALLOCATIONS_PER_SPAN) / self.span_alloc.page_size()).min(u16::MAX as usize).max(1);
                let mini_heap = MiniHeap::new(self.span_alloc.allocate_span(pages.try_into().unwrap())?, self.span_alloc.page_size(), request.size_class());
                let size_class_index = request.size_class_index();
                if let Some(old_mini_heap) = thread_heap.replace_mini_heap(request, mini_heap) {
                    let mut extra_heaps = self.extra_mini_heaps[size_class_index].lock();
                    eprintln!("Mini heap moved to global extra heap storage.");
                    extra_heaps.push(old_mini_heap).map_err(|(_, err)| err)?;
                }
                Ok(thread_heap.alloc(layout).map_err(|_| ()).expect("Mini heap was just replaced"))
            }
        }
    }

    pub unsafe fn dealloc(&self, ptr: NonNull<[u8]>, layout: Layout) -> Result<(), S::DeallocError> {
        todo!();
    }

    fn create_thread_heap(&self, thread_id: usize, thread_local: RwLockUpgradableGuard<'_, ThreadLocalLookup<R, S>>) -> Result<*mut ThreadHeap<R, S::Handle>, S::AllocError> {
        let item = (
            thread_id,
            UnsafeCell::new(ThreadHeap::new(R::seed_from_u64({ let mut rng = self.rng.lock(); rng.next_u64() }))),
        );

        let mut thread_local = thread_local.upgrade();
        let index = thread_local.push(item).map_err(|(_, err)| err)?;
        let thread_local = thread_local.downgrade();

        let slice = thread_local.as_slice();
        Ok(slice[index].1.get())
    }
}

