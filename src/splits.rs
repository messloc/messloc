use crate::list_entry::ListEntry;
use crate::mini_heap::MiniHeap;
use crate::utils;
use crate::{MAX_MERGE_SETS, MAX_SPLIT_LIST_SIZE, NUM_BINS};
use libc::c_void;

pub struct MergeSetWithSplits<'a> {
    pub merge_set: [(&'a MiniHeap<'a>, &'a MiniHeap<'a>); MAX_MERGE_SETS],
    pub left: [Option<&'a MiniHeap<'a>>; MAX_SPLIT_LIST_SIZE],
    pub right: [Option<&'a MiniHeap<'a>>; MAX_SPLIT_LIST_SIZE],
}

impl MergeSetWithSplits<'_> {
    pub unsafe fn madvise(&mut self) {
        let first = self.left.first();
    }
}

pub trait Madvisable {
    fn as_mut_ptr_of_starting_addr(&mut self) -> *mut c_void;

    fn madvise(&mut self, size: usize) {
        utils::madvise(self.as_mut_ptr_of_starting_addr() as *mut c_void, size);
    }
}

impl Madvisable for &[&MiniHeap<'_>] {
    fn as_mut_ptr_of_starting_addr(&mut self) -> *mut c_void {
        (*self.get_mut(0).unwrap()) as *const MiniHeap<'_> as *mut c_void
    }
}
