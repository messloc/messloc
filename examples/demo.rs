use std::{alloc::{Layout}, borrow::BorrowMut, ops::DerefMut};
use std::cell::*;

use messloc::blind::{span::{SpanAllocator, TestSpanAllocator}, Messloc, SpanVec, SystemSpanAlloc};

#[global_allocator]
static ALLOCATOR: Messloc = Messloc::new();

use rand::SeedableRng;

// thread_local!(static FOO: RefCell<u32> = RefCell::new(1));

fn main() {
    // println!("hello");

    // FOO.with(|f| {
    //     *f.borrow_mut() = 54;
    //     dbg!(f.borrow());
    // });
    
    // std::thread::spawn({
    //     move || {
    //         println!("hello");
    //
    //         // FOO.with(|f| {
    //         //     *f.borrow_mut() = 54;
    //         //     dbg!(f.borrow());
    //         // });
    //     }
    // }).join().unwrap();
    
    println!("start");
    let mut total = 0.0;
    for _ in 0..1000 {
        // use std::alloc::GlobalAlloc;
        // let x = unsafe { ALLOCATOR.alloc(Layout::new::<u64>()) };
        let now = std::time::Instant::now();
        // let x = "abcdefg".to_string();
        total += now.elapsed().as_secs_f64();

        // dbg!(x);
    }
    println!("{} s", total / 1000.0);

    // use rand_xoshiro::Xoshiro256Plus;
    //
    // let rng = Xoshiro256Plus::seed_from_u64(1);
    // let span_alloc = TestSpanAllocator;
    // let mut alloc = std::sync::Arc::new(messloc::blind::GlobalHeap::new(span_alloc, rng).unwrap());
    //
    // for _ in 0..10 {
    //     let x = unsafe { alloc.alloc(Layout::new::<u64>()) };
    //     dbg!(x);
    // }
    //
    // std::thread::spawn({
    //     let alloc = alloc.clone();
    //     move || {
    //         let x = unsafe { alloc.alloc(Layout::new::<u64>()) };
    //         dbg!(x);
    //     }
    // }).join().unwrap();
    //
    // std::thread::spawn({
    //     let alloc = alloc.clone();
    //     move || {
    //         let x = unsafe { alloc.alloc(Layout::new::<u64>()) };
    //         dbg!(x);
    //     }
    // }).join().unwrap();
    //
    // std::thread::spawn({
    //     let alloc = alloc.clone();
    //     move || {
    //         let x = unsafe { alloc.alloc(Layout::new::<u64>()) };
    //         dbg!(x);
    //     }
    // }).join().unwrap();
    //
    // let x = unsafe { alloc.alloc(Layout::new::<u64>()) };
    // dbg!(x);
}
