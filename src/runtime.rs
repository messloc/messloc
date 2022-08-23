use std::{
    cell::OnceCell,
    process::id,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use crate::{
    global_heap::GlobalHeap, mini_heap::MiniHeap, splits::MergeSetWithSplits, utils::madvise,
    MAX_MERGE_SETS, MAX_SPLIT_LIST_SIZE, NUM_BINS,
};

struct FastWalkTime<'a> {
    pid: u32,
    pub global_heap: GlobalHeap<'a>,
    pub merge_set: MergeSetWithSplits<'a>,
    pub free_lists: [[(MiniHeapListEntry, u64); NUM_BINS]; 3],
}

impl<'a> FastWalkTime<'a> {}

pub struct Runtime<'a>(Arc<Mutex<FastWalkTime<'a>>>);

impl<'a> Runtime<'a> {
    pub fn update_pid(&mut self) {
        todo!();
        // self.pid = id();
    }

    pub fn lock(
        &self,
    ) -> Result<MutexGuard<'_, FastWalkTime<'_>>, PoisonError<MutexGuard<'_, FastWalkTime<'_>>>>
    {
        self.0.lock()
    }

    pub fn global_heap(&self) -> &GlobalHeap<'a> {
        &(&*self.0.lock().unwrap()).global_heap
    }
}

impl PartialEq<Self> for Runtime<'_> {
    fn eq(&self, rhs: &Self) -> bool {
        // This is a hack to ensure that partial eq can be implemented on other types
        // Runtime in a singleton instance and hence can be ignored from the partialeq
        // checks

        true
    }
}
