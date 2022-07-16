use super::MiniHeap;

use super::SIZE_CLASS_COUNT;
use super::linked_heap::LinkedList;

pub struct GlobalHeap<S> {
    span_allocator: S,
    mini_heaps: [LinkedList<MiniHeap>; SIZE_CLASS_COUNT],
}

impl<S> GlobalHeap<S> {
    pub fn new(mut span_allocator: S) -> Self {
        let mini_heaps = [(); SIZE_CLASS_COUNT].map(|_| LinkedList::new(&mut span_allocator, rng));

        Self {
            span_allocator,
            mini_heaps
        }
    }
}

pub struct LocalHeap<R, P> {
    rng: R,
    global_heap: GlobalHeap<P>,
    mini_heaps: [Option<MiniHeap>; SIZE_CLASS_COUNT],
}

impl<R, P> LocalHeap<R, P> {
    pub fn new(global_heap: GlobalHeap<P>, rng: R) -> Self {
        Self {
            rng,
            global_heap,
            mini_heaps: [(); SIZE_CLASS_COUNT].map(|_| None),
        }
    }
}
