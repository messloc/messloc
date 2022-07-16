//! This is a blind implementation taken directly from the paper describing the allocator
//! https://raw.githubusercontent.com/plasma-umass/Mesh/master/mesh-pldi19-powers.pdf
//!
//! The author's orginal code was not used as reference for this implementation.

// pub mod allocation_mask;
// pub mod linked_heap;
// pub mod mini_heap;
// pub mod shuffle_vec;
pub mod span;
// pub mod local_heap;
use std::{
    alloc::{GlobalAlloc, Layout},
    mem::MaybeUninit,
    ptr::{null_mut, NonNull}, cell::UnsafeCell, marker::PhantomData, thread::ThreadId,
};

// use allocation_mask::*;
// use linked_heap::*;
// use mini_heap::*;
// use shuffle_vec::*;
use span::*;
use spin::{Mutex, Lazy, RwLock};
use thread_local::ThreadLocal;

const SIZE_CLASS_COUNT: usize = 25;

static SIZE_CLASSES: [usize; SIZE_CLASS_COUNT] = [
    8, 16, 32, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896,
    1024, 2048, 4096, 8192, 16384,
];

struct ThreadHeap;

#[derive(Debug)]
pub struct SpanVec<T, H> {
    span: Span<H>,
    phantom: PhantomData<*const [T]>,
    length: usize,
    capacity: usize,
}

unsafe impl<T: Send, H: Send> Send for SpanVec<T, H> {}
unsafe impl<T: Sync, H: Sync> Sync for SpanVec<T, H> {}

impl<T, H> SpanVec<T, H> {
    pub unsafe fn with_capacity<S: SpanAllocator<Handle=H>>(mut span_alloc: &mut S, capacity: usize) -> Result<Self, ()> {
        let page_size = span_alloc.page_size();
        let pages = (capacity * core::mem::size_of::<T>() + page_size - 1) / page_size;
        let pages = pages.max(1);
        let span = span_alloc.allocate_span(pages.try_into().unwrap()).map_err(|_| ())?;
        
        let capacity = (pages * page_size) / core::mem::size_of::<T>();

        Ok(Self {
            span,
            phantom: PhantomData,
            length: 0,
            capacity,
        })
    }

    pub unsafe fn add_more_capacity<S: SpanAllocator<Handle=H>>(&mut self, mut span_alloc: &mut S, extra_capacity: usize) -> Result<(), ()> {
        let page_size = span_alloc.page_size();
        let pages = ((self.capacity + extra_capacity) * core::mem::size_of::<T>() + page_size - 1) / page_size;
        let pages = pages.max(1);
        let span = span_alloc.allocate_span(pages.try_into().unwrap()).map_err(|_| ())?;
        
        let capacity = (pages * page_size) / core::mem::size_of::<T>();

        core::ptr::copy_nonoverlapping(self.span.data_ptr().as_ptr().cast::<T>(), span.data_ptr().as_ptr().cast::<T>(), self.length);

        let old_span = core::mem::replace(&mut self.span, span);

        span_alloc.deallocate_span(old_span).map_err(|_| ());

        Ok(())
    }

    pub fn push(&mut self, value: T) -> Option<T> {
        if self.length == self.capacity {
            return Some(value);
        }

        let pointer: *mut T = unsafe { self.span.data_ptr().as_ptr().cast::<T>().offset(self.length as isize) };

        unsafe { pointer.write(value) };

        self.length += 1;

        None
    }

    pub fn as_slice(&self) -> &[T] {
        unsafe { core::slice::from_raw_parts(self.span.data_ptr().as_ptr().cast(), self.length) }
    }

    pub fn drop_items<F: Fn(&mut T)>(self, custom_drop: F) -> Span<H> {
        for offset in 0..self.length {
            let item = unsafe { self.span.data_ptr().as_ptr().cast::<T>().offset(offset as isize) };
            custom_drop(unsafe { &mut *item });
            unsafe { core::ptr::drop_in_place(item) };
        }

        self.span
    }
}

struct GlobalHeap<S> {
    span_alloc: Mutex<S>,
    thread_local: RwLock<SpanVec<(ThreadId, UnsafeCell<u32>), i32>>,
}

unsafe impl<S> Sync for GlobalHeap<S> {}

pub struct Messloc {
    heap: Lazy<GlobalHeap<SystemSpanAlloc>>,
}

impl Messloc {
    pub const fn new() -> Self {
        Self {
            heap: Lazy::new(|| {
                let mut span_alloc = SystemSpanAlloc::get();
                let thread_local = unsafe { SpanVec::with_capacity(&mut span_alloc, 0) }.unwrap();

                GlobalHeap {
                    span_alloc: Mutex::new(span_alloc),
                    thread_local: RwLock::new(thread_local),
                }
            })
        }
    }

    pub fn test_alloc(&self) -> &mut u32 {
        let thread_id = std::thread::current().id();

        let thread_local = self.heap.thread_local.upgradeable_read();
        if let Some(item) = thread_local.as_slice().iter().find(|(item_id, _)| *item_id == thread_id) {
            unsafe { &mut *item.1.get() }
        } else {
            let mut thread_local = thread_local.upgrade();
            thread_local.push((thread_id, UnsafeCell::new(0)));
            let slice = thread_local.as_slice();
            unsafe { &mut *slice[slice.len() - 1].1.get() }
        }
    }
}

unsafe impl GlobalAlloc for Messloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut span_alloc = self.heap.span_alloc.lock();
        if let Ok(span) = span_alloc.allocate_span(1) {
            return span.data_ptr().as_ptr();
        }
        null_mut()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        let span = unsafe { Span::new(NonNull::new(ptr).unwrap(), -1, 1) };

        let mut span_alloc = self.heap.span_alloc.lock();
        span_alloc.deallocate_span(span);
    }
}

pub struct SystemSpanAlloc(Lazy<usize>);

impl SystemSpanAlloc {
    pub const fn get() -> Self {
        SystemSpanAlloc(Lazy::new(|| unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }))
    }
}

unsafe impl SpanAllocator for SystemSpanAlloc {
    type AllocError = ();
    type DeallocError = ();
    type MergeError = ();
    type Handle = i32;

    fn page_size(&self) -> usize {
        *self.0
    }

    unsafe fn allocate_span(&mut self, pages: u16) -> Result<Span<Self::Handle>, ()> {
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

        eprintln!("Allocated pages: {}", pages);

        Ok(unsafe { Span::new(NonNull::new(pointer).unwrap().cast(), fd, pages) })
    }

    unsafe fn deallocate_span(&mut self, span: Span<Self::Handle>) -> Result<(), ()> {
        eprintln!("Deallocated pages: {} {} {:?}", span.pages(), span.handle(), span.state());

        let result = libc::munmap(span.data_ptr().as_ptr().cast(), span.pages() as usize * self.page_size());

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

    unsafe fn merge_spans(&mut self, span: &Span<Self::Handle>, span_to_merge: &mut Span<Self::Handle>) -> Result<(), ()> {
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
