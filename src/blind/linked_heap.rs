use std::{ptr::NonNull, mem::MaybeUninit, alloc::Layout};

use crate::blind::span::TestSpanAllocator;

use super::MiniHeap;

use super::span::SpanAllocator;



#[derive(Debug)]
enum Nested {
    Reserved(NonNull<MaybeUninit<LinkedMiniHeap>>),
    Exists(NonNull<LinkedMiniHeap>),
}

#[derive(Debug)]
struct LinkedMiniHeap {
    // Root mini heap to store other mini heaps
    storage: MiniHeap,

    /// Linked list of nested mini heaps
    next: Nested,
}

fn pages_needed<T>(count: usize) -> u32 {
    let page_size = page_size::get();
    (((core::mem::size_of::<T>() * count) + page_size - 1) / page_size) as u32
}

impl LinkedMiniHeap {
    fn new<T: SpanAllocator, R: rand::Rng>(span_alloc: &mut T, rng: &mut R) -> Self {
        let span = span_alloc.allocate_span(pages_needed::<LinkedMiniHeap>(5));
        let span = span.assign_size_class(core::mem::size_of::<LinkedMiniHeap>() as u16, 8);
        let mut storage = MiniHeap::new(span, rng);

        let reserved = storage.alloc(Layout::new::<LinkedMiniHeap>()).unwrap();

        Self {
            storage,
            next: Nested::Reserved(reserved.cast()),
        }
    }

    fn alloc<T: SpanAllocator, R: rand::Rng>(&mut self, layout: Layout, span_alloc: &mut T, rng: &mut R) -> NonNull<u8> {
        assert!(layout.size() <= self.storage.span().size_class() as usize);
        // check if there is space in the local heap
        if let Some(pointer) = self
            .storage
            .alloc(layout)
        {
            pointer
        } else {
            // the current heap doesn't have any more room
            match self.next {
                Nested::Exists(mut pointer) => {
                    unsafe { pointer.as_mut() }.alloc(layout, span_alloc, rng)
                }
                Nested::Reserved(mut pointer) => {
                    let node = LinkedMiniHeap::new(span_alloc, rng);
                    unsafe { pointer.as_mut() }.write(node);
                    let mut pointer : NonNull<LinkedMiniHeap> = pointer.cast();
                    self.next = Nested::Exists(pointer);
                    unsafe { pointer.as_mut() }.alloc(layout, span_alloc, rng)
                }
            }
        }
    }
}

#[test]
fn test_linked() {
    let mut rng = rand::thread_rng();
    let mut span_alloc = TestSpanAllocator;
    let mut list = LinkedMiniHeap::new(&mut span_alloc, &mut rng);
    dbg!(&list);

    // for _ in 0..100 {
    //     let mut pointer: NonNull<[u32; 4]> = list.alloc(Layout::new::<[u32; 4]>(), &mut span_alloc, &mut rng).cast();
    //     let mut x = unsafe { pointer.as_mut() };
    //     x[0] = 1;
    //     x[1] = 2;
    //     x[2] = 3;
    //     x[3] = 4;
    //     dbg!(x);
    // }
    //
    // panic!();
}

struct Node<T> {
    value: T,
    next: Option<NonNull<Node<T>>>,
}

pub struct LinkedList<T> {
    storage: LinkedMiniHeap,
    head: Option<NonNull<Node<T>>>,
}

impl<T> core::fmt::Debug for LinkedList<T> where T: core::fmt::Debug {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.iter())
            .finish()
    }
}

impl<T> LinkedList<T> {
    pub fn new<S: SpanAllocator, R: rand::Rng>(span_alloc: &mut S, rng: &mut R) -> Self {
        Self {
            storage: LinkedMiniHeap::new(span_alloc, rng),
            head: None,
        }
    }

    pub fn iter<'a>(&'a self) -> LinkedListIter<'a, T> {
        LinkedListIter { next: self.head.clone(), phantom: core::marker::PhantomData }
    }

    pub fn push<S: SpanAllocator, R: rand::Rng>(&mut self, value: T, span_alloc: &mut S, rng: &mut R) {
        let mut pointer: NonNull<MaybeUninit<Node<T>>> = self.storage.alloc(Layout::new::<Node<T>>(), span_alloc, rng).cast();
        assert_eq!(pointer.as_ptr() as usize % core::mem::align_of::<T>(), 0);
        unsafe { pointer.as_mut().write(Node {
            next: self.head,
            value,
        }) };
        self.head = Some(pointer.cast())
    }
}

#[derive(Debug)]
pub struct LinkedListIter<'a, T> {
    phantom: core::marker::PhantomData<&'a LinkedMiniHeap>,
    next: Option<NonNull<Node<T>>>
}

impl<'a, T: 'a> Iterator for LinkedListIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        if let Some(item) = self.next.take() {
            self.next = unsafe { item.as_ref() }.next;
            Some(&unsafe { item.as_ref() }.value)
        } else {
            None
        }
    }
}

#[test]
fn test_list() {
    let mut rng = rand::thread_rng();
    let mut span_alloc = TestSpanAllocator;
    let mut list: LinkedList<u32> = LinkedList::new(&mut span_alloc, &mut rng);
    
    for i in 0..100 {
        list.push(i, &mut span_alloc, &mut rng);
    }

    dbg!(list);

    panic!();
}
