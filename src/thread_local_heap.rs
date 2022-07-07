use std::cell::{RefCell, UnsafeCell};

use crate::global_heap::GlobalHeap;

pub struct ThreadLocalHeap;

impl ThreadLocalHeap {
    pub fn get() -> &'static Self {
        // hand-rolled `OnceCell`
        thread_local! {
            static TLH: UnsafeCell<Option<ThreadLocalHeap>> = UnsafeCell::new(None);
        }
        TLH.with(|tlh| {
            if let Some(tlh) = unsafe { &*tlh.get() }.as_ref() {
                return tlh;
            }
            let val = ThreadLocalHeap;
            unsafe { *tlh.get() = Some(val) }
            unsafe { &*tlh.get() }.as_ref().unwrap()
        })
    }
}
