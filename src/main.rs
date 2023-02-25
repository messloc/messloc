#![feature(once_cell)]
#![deny(clippy::pedantic)]
use messloc::MessyLock;
extern crate alloc;

#[cfg_attr(not(test), global_allocator)]
static ALLOCATOR: MessyLock = MessyLock(once_cell::sync::OnceCell::new());
pub fn main() {
}
