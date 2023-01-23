#![feature(once_cell)]
use messloc::MessyLock;
extern crate alloc;

#[global_allocator]
static ALLOCATOR: MessyLock = MessyLock(once_cell::sync::OnceCell::new());
pub fn main() {
    let i = 1;
}
