#![feature(once_cell)]
use messloc::MessyLock;
extern crate alloc;

#[cfg_attr(not(test), global_allocator)]
static ALLOCATOR: MessyLock = MessyLock(once_cell::sync::OnceCell::new());
pub fn main() {
    let i = 5;
}
