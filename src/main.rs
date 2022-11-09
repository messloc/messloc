#![feature(once_cell)]
use messloc::{Messloc, MessyLock};
use std::sync::LazyLock;

// #[global_allocator]
// static ALLOCATOR : MessyLock = MessyLock(LazyLock::new(|| Messloc::init()));

fn main() {
    stacker::grow(16 * 1024 * 1024 * 1024, || {
        let wtf = Messloc::init();
    });
}
