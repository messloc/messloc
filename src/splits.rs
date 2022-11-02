use crate::list_entry::ListEntry;
use crate::mini_heap::MiniHeap;
use crate::utils;
use crate::{MAX_MERGE_SETS, MAX_SPLIT_LIST_SIZE, NUM_BINS};
use libc::c_void;
use std::cell::RefCell;
use std::mem::MaybeUninit;
use std::rc::Rc;

#[allow(clippy::module_name_repetitions)]
pub struct MergeSetWithSplits {
    pub merge_set: [Option<(*mut MiniHeap, *mut MiniHeap)>; MAX_MERGE_SETS],
    pub left: [Option<*mut MiniHeap>; MAX_SPLIT_LIST_SIZE],
    pub right: [Option<*mut MiniHeap>; MAX_SPLIT_LIST_SIZE],
}

impl MergeSetWithSplits {
    pub unsafe fn madvise(&mut self) {
        let first = self.left.first();
    }
}

impl Default for MergeSetWithSplits {
    fn default() -> Self {
        Self {
            merge_set: std::array::from_fn(|_| None),
            left: std::array::from_fn(|_| None),
            right: std::array::from_fn(|_| None),
        }
    }
}

#[allow(clippy::missing_safety_doc)]
pub(crate) unsafe trait Madvisable {
    fn as_mut_ptr_of_starting_addr(&mut self) -> *mut c_void;

    unsafe fn madvise(&mut self, size: usize) {
        utils::madvise(self.as_mut_ptr_of_starting_addr() as *mut c_void, size).unwrap();
    }
}

unsafe impl Madvisable for &mut [&MiniHeap] {
    fn as_mut_ptr_of_starting_addr(&mut self) -> *mut c_void {
        (*self.get_mut(0).unwrap()) as *const MiniHeap as *mut c_void
    }
}

unsafe impl Send for MergeSetWithSplits {}
