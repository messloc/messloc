use crate::list_entry::ListEntry;
use crate::mini_heap::MiniHeap;
use crate::utils;
use crate::{MAX_MERGE_SETS, MAX_SPLIT_LIST_SIZE, NUM_BINS};
use libc::c_void;
use std::cell::RefCell;
use std::mem::MaybeUninit;
use std::ptr::null_mut;
use std::rc::Rc;

pub struct MergeElement {
    pub mini_heap: *mut MiniHeap,
    pub direction: SplitType,
}

#[allow(clippy::module_name_repetitions)]
pub struct MergeSetWithSplits<const N: usize>(pub [MergeElement; N]);

pub enum SplitType {
    MergedWith(*mut MiniHeap),
    Left,
    Right,
}

impl<const N: usize> MergeSetWithSplits<N> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(std::array::from_fn(|_| MergeElement {
            mini_heap: null_mut(),
            direction: SplitType::Left,
        }))
    }
    pub unsafe fn madvise(&mut self) {
        todo!()
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

unsafe impl<const N: usize> Send for MergeSetWithSplits<N> {}
