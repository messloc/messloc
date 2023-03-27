use core::alloc::Layout;
use spin::Mutex;

use crate::global_heap::GlobalHeap;

pub struct FastWalkTime {
    pub signal_fd: i32,
    pub global_heap: GlobalHeap,
}

pub struct Messloc(pub Mutex<FastWalkTime>);

impl Messloc {
    #[must_use]
    pub fn init() -> Self {
        Self(Mutex::new(FastWalkTime {
            signal_fd: 0,
            global_heap: GlobalHeap::init(),
        }))
    }

    #[allow(clippy::missing_safety_doc)]
    #[must_use]
    pub unsafe fn allocate(&self, layout: Layout) -> *mut u8 {
        let heap = &mut self.0.lock().global_heap;
        heap.malloc(layout.size()) as *mut u8
    }

    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn deallocate(&self, ptr: *mut u8, layout: Layout) {
        self.0
            .lock()
            .global_heap
            .free(ptr as *mut (), layout.size());
    }
}

impl PartialEq<Self> for Messloc {
    fn eq(&self, _rhs: &Self) -> bool {
        // This is a hack to ensure that partial eq can be implemented on other types
        // Runtime in a singleton instance and hence can be ignored from the partialeq
        // checks

        true
    }
}

impl Drop for Messloc {
    fn drop(&mut self) {}
}
