use std::alloc::Layout;

use messloc::blind::{Messloc, SystemSpanAlloc, SpanVec, span::SpanAllocator};

#[global_allocator]
static ALLOCATOR: Messloc = Messloc::new();

fn main() {
    unsafe {
        let mut alloc = SystemSpanAlloc::get();

        let x = alloc.allocate_span(1).unwrap();
        dbg!(&x);

        let mut y = alloc.allocate_span(1).unwrap();
        dbg!(&y);

        x.data_ptr().as_ptr().cast::<u32>().write(123);
        y.data_ptr().as_ptr().cast::<u32>().write(456);

        dbg!(*x.data_ptr().as_ptr().cast::<u32>());
        dbg!(*y.data_ptr().as_ptr().cast::<u32>());

        alloc.merge_spans(&x, &mut y).unwrap();

        dbg!(&x, &y);

        dbg!(*x.data_ptr().as_ptr().cast::<u32>());
        dbg!(*y.data_ptr().as_ptr().cast::<u32>());

        y.data_ptr().as_ptr().cast::<u32>().write(789);

        dbg!(*x.data_ptr().as_ptr().cast::<u32>());
        dbg!(*y.data_ptr().as_ptr().cast::<u32>());

        x.data_ptr().as_ptr().cast::<u32>().write(42);

        dbg!(*x.data_ptr().as_ptr().cast::<u32>());
        dbg!(*y.data_ptr().as_ptr().cast::<u32>());

        alloc.deallocate_span(y);
        alloc.deallocate_span(x);
        
        std::thread::sleep(std::time::Duration::from_secs_f32(60.0));
    }

    // println!("test");
    //
    // dbg!(std::thread::current().id());
    //
    // let mut test: SpanVec<u32, _> = unsafe { SpanVec::with_capacity(&mut SystemSpanAlloc::get(), 10) }.unwrap();
    // dbg!(&test);
    // dbg!(test.as_slice());
    // test.push(12);
    // test.push(123);
    // test.push(1234);
    // dbg!(test.as_slice());
    //
    // unsafe { test.add_more_capacity(&mut SystemSpanAlloc::get(), 10) };
    // dbg!(&test);
    //
    // std::thread::sleep(std::time::Duration::from_secs_f32(60.0));
    //
    // // for x in 0..10000 {
    // //     test.push(x);
    // // }
    // let span = test.drop_items(|item| { dbg!(item); });
    // unsafe { SystemSpanAlloc::get().deallocate_span(span) };
    //
    // *dbg!(ALLOCATOR.test_alloc()) = 101;
    // *dbg!(ALLOCATOR.test_alloc()) = 42;
    // dbg!(ALLOCATOR.test_alloc());
    //
    // std::thread::spawn(move || {
    //     *dbg!(ALLOCATOR.test_alloc()) = 123;
    //     dbg!(ALLOCATOR.test_alloc());
    //     std::thread::sleep(std::time::Duration::from_secs_f32(60.0));
    // }).join();
    //
    // dbg!(ALLOCATOR.test_alloc());

    // let mut x = "test".to_string();
    // x.push('x');
    // dbg!(x);
}
