use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use crate::global_heap::List;
use crate::mini_heap::FreeListId;
use crate::mini_heap::{AtomicMiniHeapId, MiniHeap, MiniHeapId};
use crate::runtime::Runtime;

#[derive(PartialEq)]
pub struct ListEntry<'a> {
    runtime: Runtime<'a>,
    pub prev: Option<AtomicMiniHeapId<MiniHeap<'a>>>,
    pub next: Option<AtomicMiniHeapId<MiniHeap<'a>>>,
}

impl ListEntry<'_> {
    pub fn new(
        runtime: Runtime<'_>,
        prev: Option<AtomicMiniHeapId<MiniHeap<'_>>>,
        next: Option<AtomicMiniHeapId<MiniHeap<'_>>>,
    ) -> Self {
        ListEntry {
            runtime: runtime.share(),
            prev,
            next,
        }
    }

    pub fn set_next(&mut self, next: AtomicMiniHeapId<MiniHeap<'_>>) {
        self.next = Some(next);
    }

    pub fn set_prev(&mut self, prev: AtomicMiniHeapId<MiniHeap<'_>>) {
        self.prev = Some(prev);
    }

    pub fn add(
        &self,
        list_head: ListEntry<'_>,
        list_id: u32,
        self_id: AtomicMiniHeapId<MiniHeap<'_>>,
        new: &mut MiniHeap<'_>,
    ) {
        let old_id = new.get_free_list_id();
        assert!(!new.is_large_alloc());

        let new_free_list = new.get_free_list();
        if new_free_list.next.is_some() {
            new_free_list.remove(list_head);
        }

        new.set_free_list_id(FreeListId::from_integer(list_id));

        let new_id = self
            .runtime
            .global_heap()
            .mini_heap_for(new as *const MiniHeap<'_> as *mut ());
        if self.prev.unwrap().is_head() {
            self.set_next(AtomicMiniHeapId::new(new_id));
        } else {
            let prev_list = unsafe {
                self.runtime
                    .global_heap()
                    .mini_heap_for_id(self.prev.unwrap())
                    .as_ref()
                    .unwrap()
                    .get_free_list()
            };
            prev_list.set_next(AtomicMiniHeapId::new(new_id));
        }

        new.set_free_list(ListEntry::new(
            self.runtime.share(),
            self.prev,
            Some(self_id),
        ));
        self.set_prev(AtomicMiniHeapId::new(new_id));
    }

    pub fn remove(&self, list_head: ListEntry<'_>) {
        if let Some(prev_id) = self.prev && let Some(next_id) = self.next {
           let mh = unsafe { self.runtime.global_heap().mini_heap_for_id(next_id).as_ref().unwrap() };
            let prev = if prev_id.is_head() {
                &list_head
            } else {
                mh.get_free_list()
            };

            let next = if next_id.is_head() {
                &list_head
            } else {
                mh.get_free_list()
            };

            prev.set_next(next_id);
            prev.set_prev(prev_id);

            self.prev = None;
            self.next = None;
       }
    }
}

pub trait Listable: PartialEq + Sized {
    fn get_free_list(&self) -> ListEntry<'_>;
    fn set_free_list(&mut self, free_list: ListEntry<'_>);
    fn get_free_list_id(&self) -> FreeListId;
    fn set_free_list_id(&mut self, free_list: FreeListId);
    fn is_large_alloc(&self) -> bool;
}
