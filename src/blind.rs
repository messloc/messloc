//! This is a blind implementation taken directly from the paper describing the allocator
//! https://raw.githubusercontent.com/plasma-umass/Mesh/master/mesh-pldi19-powers.pdf
//!
//! The author's orginal code was not used as reference for this implementation.

pub mod allocation_mask;
// pub mod linked_heap;
pub mod mini_heap;
pub mod shuffle_vec;
pub mod span;
pub mod span_vec;
// pub mod local_heap;
use std::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ptr::{null_mut, NonNull, slice_from_raw_parts_mut},
    thread::ThreadId,
};

use allocation_mask::*;
use rand::SeedableRng;
// use linked_heap::*;
use mini_heap::*;
use rand::Rng;
use shuffle_vec::*;
use span::*;
pub use span_vec::*;
use spin::{Lazy, Mutex, Once, RwLock, RwLockReadGuard, RwLockUpgradableGuard};
use thread_local::ThreadLocal;

const SIZE_CLASS_COUNT: usize = 25;

static SIZE_CLASSES: [u16; SIZE_CLASS_COUNT] = [
    8, 16, 32, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896,
    1024, 2048, 4096, 8192, 16384,
];

struct ThreadHeap<R, H> {
    rng: R,
    shuffle_vec: ShuffleVector<MAX_ALLOCATIONS_PER_SPAN>,
    mini_heaps: [Option<MiniHeap<H>>; SIZE_CLASS_COUNT],
}

unsafe impl<R, H> Send for ThreadHeap<R, H> {}

type ThreadLocalLookup<R, S> = SpanVec<(ThreadId, UnsafeCell<ThreadHeap<R, <S as SpanAllocator>::Handle>>), S>;

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

use rand_xoshiro::Xoshiro256Plus;

pub struct Messloc {
    heap: Lazy<GlobalHeap<Xoshiro256Plus, SystemSpanAlloc>>,
}

impl<R: Rng + SeedableRng, S: SpanAllocator> GlobalHeap<R, S> {
    pub unsafe fn alloc(&self, layout: Layout) -> Result<NonNull<[u8]>, S::AllocError> {
        // check if the size is over 16k
        if layout.size() > SIZE_CLASSES[SIZE_CLASS_COUNT - 1] as usize {
            // use large alloc strategy
            todo!()
        }

        let thread_id = std::thread::current().id();

        let thread_local = self.thread_local.upgradeable_read();
        let thread_heap = if let Some(item) = thread_local
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
        Ok(NonNull::new(slice_from_raw_parts_mut(NonNull::dangling().as_ptr(), 0)).unwrap())
    }

    fn create_thread_heap(&self, thread_id: ThreadId, thread_local: RwLockUpgradableGuard<'_, ThreadLocalLookup<R, S>>) -> Result<*mut ThreadHeap<R, S::Handle>, S::AllocError> {
        let item = (
            thread_id,
            UnsafeCell::new(ThreadHeap {
                shuffle_vec: ShuffleVector::<MAX_ALLOCATIONS_PER_SPAN>::new(),
                // mini_heaps: array_init::try_array_init(|index| -> Result<_, S::AllocError> {
                //     let size_class = SIZE_CLASSES[index];
                //     Ok(MiniHeap::new(unsafe { self.span_alloc.allocate_span(1)? }, size_class))
                // })?,
                mini_heaps: SIZE_CLASSES.map(|_| None),
                rng: R::seed_from_u64({ let mut rng = self.rng.lock(); rng.next_u64() }),
            }),
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

impl Messloc {
    pub const fn new() -> Self {
        Self {
            heap: Lazy::new(|| {
                let mut span_alloc = SystemSpanAlloc::get();
                let thread_local =
                    unsafe { SpanVec::with_capacity_in(0, span_alloc.clone()) }.unwrap();

                GlobalHeap {
                    rng: Mutex::new(Xoshiro256Plus::seed_from_u64(1234568123987)),
                    span_alloc,
                    thread_local: RwLock::new(thread_local),
                }
            }),
        }
    }
}

unsafe impl GlobalAlloc for Messloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Ok(span) = self.heap.alloc(layout) {
            span.as_ptr().cast()
        } else {
            null_mut()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        eprintln!("dealloc!");
        // let span = unsafe { Span::new(NonNull::new(ptr).unwrap(), -1, 1) };
        //
        // self.heap.span_alloc.deallocate_span(&span);
    }
}

pub struct SystemSpanAlloc(Once<usize>);

unsafe impl Send for SystemSpanAlloc {}
unsafe impl Sync for SystemSpanAlloc {}

impl Clone for SystemSpanAlloc {
    fn clone(&self) -> Self {
        Self(Once::initialized(self.page_size()))
    }
}

impl SystemSpanAlloc {
    pub const fn get() -> Self {
        SystemSpanAlloc(Once::new())
    }
}

unsafe impl SpanAllocator for SystemSpanAlloc {
    type AllocError = ();
    type DeallocError = ();
    type MergeError = ();
    type Handle = i32;

    fn page_size(&self) -> usize {
        *self
            .0
            .call_once(|| unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize })
    }

    unsafe fn allocate_span(&self, pages: u16) -> Result<Span<Self::Handle>, ()> {
        eprintln!("Allocated pages: {}", pages);

        let name = &['s' as u8, 0];
        let fd = libc::memfd_create(name as *const u8 as *const i8, libc::MFD_CLOEXEC);

        if fd == -1 {
            return Err(());
        }

        let result = libc::ftruncate(fd, (self.page_size() * pages as usize) as i64);

        if result != 0 {
            eprintln!("Failed to size fd!");
            return Err(());
        }

        let pointer = unsafe {
            libc::mmap(
                null_mut(),
                self.page_size() * pages as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if pointer == libc::MAP_FAILED {
            eprintln!("Allocated errored!");
            return Err(());
        }

        Ok(unsafe { Span::new(NonNull::new(pointer).unwrap().cast(), fd, pages) })
    }

    unsafe fn deallocate_span(&self, span: &Span<Self::Handle>) -> Result<(), ()> {
        eprintln!(
            "Deallocated pages: {} {} {:?}",
            span.pages(),
            span.handle(),
            span.state()
        );

        let result = libc::munmap(
            span.data_ptr().as_ptr().cast(),
            span.pages() as usize * self.page_size(),
        );

        if result != 0 {
            eprintln!("Deallocated errored!");
            return Err(());
        }

        if span.state() == State::Normal && *span.handle() != -1 {
            if libc::close(*span.handle()) != 0 {
                eprintln!("Close errored in dealloc! {}", span.handle());
                eprintln!("{}", std::io::Error::last_os_error());
                return Err(());
            } else {
                eprintln!("Closed file {}", span.handle());
            }
        }

        Ok(())
    }

    unsafe fn merge_spans(
        &self,
        span: &Span<Self::Handle>,
        span_to_merge: &mut Span<Self::Handle>,
    ) -> Result<(), ()> {
        if span.state() != State::Normal {
            return Err(());
        }

        if span_to_merge.state() != State::Normal {
            return Err(());
        }

        let pointer = unsafe {
            libc::mmap(
                span_to_merge.data_ptr().as_ptr().cast(),
                self.page_size() * span_to_merge.pages() as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_FIXED,
                *span.handle(),
                0,
            )
        };

        if pointer == libc::MAP_FAILED {
            eprintln!("Remap errored!");
            return Err(());
        }

        if pointer != span_to_merge.data_ptr().as_ptr().cast() {
            eprintln!("Remap moved!");
            return Err(());
        }

        if *span_to_merge.handle() != -1 && libc::close(*span_to_merge.handle()) != 0 {
            eprintln!("Close errored in merge!");
            return Err(());
        }

        span_to_merge.set_state(State::Merged);
        span_to_merge.set_handle(*span.handle());

        Ok(())
    }
}
