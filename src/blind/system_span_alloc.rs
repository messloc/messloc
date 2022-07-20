use std::ptr::NonNull;
use std::ptr::null_mut;

use spin::Once;

use crate::blind::span::State;

use super::span::SpanAllocator;
use super::span::Span;

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
