use std::alloc::{Layout};

use messloc::blind::{span::{SpanAllocator, TestSpanAllocator}, Messloc, SpanVec, SystemSpanAlloc};

// #[global_allocator]
// static ALLOCATOR: Messloc = Messloc::new();

use rand::SeedableRng;

fn main() {
    println!("hello");

    use rand_xoshiro::Xoshiro256Plus;

    let rng = Xoshiro256Plus::seed_from_u64(1);
    let span_alloc = TestSpanAllocator;
    let mut alloc = std::sync::Arc::new(messloc::blind::GlobalHeap::new(span_alloc, rng).unwrap());

    let x = unsafe { alloc.alloc(Layout::new::<u64>()) };
    dbg!(x);

    std::thread::spawn({
        let alloc = alloc.clone();
        move || {
            let x = unsafe { alloc.alloc(Layout::new::<u64>()) };
            dbg!(x);
        }
    }).join().unwrap();

    std::thread::spawn({
        let alloc = alloc.clone();
        move || {
            let x = unsafe { alloc.alloc(Layout::new::<u64>()) };
            dbg!(x);
        }
    }).join().unwrap();

    std::thread::spawn({
        let alloc = alloc.clone();
        move || {
            let x = unsafe { alloc.alloc(Layout::new::<u64>()) };
            dbg!(x);
        }
    }).join().unwrap();

    let x = unsafe { alloc.alloc(Layout::new::<u64>()) };
    dbg!(x);
}
