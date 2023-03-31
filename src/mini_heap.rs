use core::{ptr::null_mut, sync::atomic::AtomicU64};

use crate::{comparatomic::Comparatomic, meshable_arena::Page};

pub struct MiniHeap {
    pub arena_begin: *mut Page,
    pub object_size: usize,
    pub span_start: *mut Self,
    pub current: Comparatomic<AtomicU64>,
}

impl MiniHeap {
    pub unsafe fn new(start: *mut (), object_size: usize) -> Self {
        MiniHeap {
            arena_begin: start.cast(),
            object_size,
            span_start: null_mut(),
            current: Comparatomic::new(0),
        }
    }
}

impl core::fmt::Debug for MiniHeap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        write!(f, "Miniheap debuggaa {:?}", self.arena_begin)
    }
}
impl PartialEq for MiniHeap {
    fn eq(&self, other: &Self) -> bool {
        self.object_size == other.object_size
            && self.span_start == other.span_start
            && self.current == other.current
    }
}

impl Drop for MiniHeap {
    fn drop(&mut self) {
        dbg!("mini heap going down");
    }
}

#[cfg(test)]
mod tests {
    use super::MiniHeap;

    #[test]
    pub fn test_dyn_array_of_mini_heaps() {
        let mut h = crate::fake_std::dynarray::DynArray::<MiniHeap, 32>::create();
        let slice = unsafe { h.as_mut_slice() };
    }
}
