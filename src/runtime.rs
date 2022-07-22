use crate::global_heap::GlobalHeap;
use std::process::id;

pub struct Runtime {
    pub heap: GlobalHeap,
    pid: u32,
}

impl Runtime {
    pub fn update_pid(&self) {
        self.pid = id();
    }
}
