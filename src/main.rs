#![feature(once_cell)]
use messloc::{Messloc, MessyLock};

extern crate alloc;

#[global_allocator]
static ALLOCATOR: MessyLock = MessyLock(once_cell::sync::OnceCell::new());

fn main() {
    let a = vec![1u8, 2, 3];
    dbg!("here");
}
