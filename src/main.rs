#![feature(once_cell)]
use messloc::{Messloc, MessyLock};

#[global_allocator]
static ALLOCATOR: MessyLock = MessyLock(once_cell::sync::Lazy::new(|| {
    let messloc = Messloc::init();
    messloc
}));

fn main() {
    let _ = vec![1u8, 2, 3];
}
