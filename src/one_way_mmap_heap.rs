use crate::PAGE_SIZE;
use core::ptr::null_mut;
use libc::{mmap, MAP_ANONYMOUS, MAP_FAILED, MAP_NORESERVE, MAP_PRIVATE, PROT_READ, PROT_WRITE};

pub struct OneWayMmapHeap;

impl OneWayMmapHeap {
    unsafe fn map(&mut self, mut size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut () {
        if size == 0 {
            return null_mut();
        }

        // Round up to the size of a page.
        size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        //TODO:: use utils::mmap instead as we are currently using two
        //different forms of mmap
        let ptr = mmap(null_mut(), size, PROT_READ | PROT_WRITE, flags, fd, 0);
        assert!(ptr != MAP_FAILED);

        ptr.cast()
    }

    pub unsafe fn malloc(&mut self, size: usize) -> *mut () {
        self.map(size, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1)
    }
}
