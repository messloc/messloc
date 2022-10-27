use std::marker::PhantomData;
use std::ptr::addr_of;
use std::sync::{atomic::Ordering, Arc, Mutex};

use crate::atomic_enum::AtomicOption;
use crate::mini_heap::FreeListId;
use crate::mini_heap::{AtomicMiniHeapId, MiniHeap, MiniHeapId};
use crate::runtime::Runtime;

#[derive(Default)]
pub struct ListEntry {
    pub prev: AtomicOption<AtomicMiniHeapId>,
    pub next: AtomicOption<AtomicMiniHeapId>,
}

impl ListEntry {
    pub fn new(prev: AtomicOption<AtomicMiniHeapId>, next: AtomicOption<AtomicMiniHeapId>) -> Self {
        ListEntry { prev, next }
    }

    pub fn add(
        &mut self,
        list_head: *mut (),
        list_id: u32,
        self_id: AtomicMiniHeapId,
        mut new: *mut (),
    ) {
        let new = unsafe { new.cast::<MiniHeap>().as_mut().unwrap() };
        let old_id = new.get_free_list_id();
        assert!(!new.is_large_alloc());

        let fl = new.free_list.borrow();

        fl.remove(list_head);
        new.set_free_list_id(FreeListId::from_integer(list_id));

        unsafe {
            match &self.prev.load(Ordering::AcqRel) {
                Some(p) if p.is_head() => {
                    self.next.store_unwrapped(
                        AtomicMiniHeapId::new(new as *const _ as *mut ()),
                        Ordering::AcqRel,
                    );
                }
                val @ Some(p) => {
                    let mem = self.prev.load(Ordering::AcqRel).unwrap();
                    let prev_list = unsafe {
                        mem.load(Ordering::AcqRel)
                            .cast::<MiniHeap>()
                            .as_ref()
                            .unwrap()
                            .free_list
                            .borrow()
                    };
                    prev_list.next.store_unwrapped(
                        AtomicMiniHeapId::new(new as *const _ as *mut ()),
                        Ordering::AcqRel,
                    );
                }
                _ => todo!(),
            }
        }

        let p = unsafe { self.prev.load_unwrapped(Ordering::AcqRel) };

        new.set_free_list(ListEntry::new(
            AtomicOption::new(p),
            AtomicOption::new(self_id),
        ));
        self.prev.store_unwrapped(
            AtomicMiniHeapId::new(new as *const _ as *mut ()),
            Ordering::AcqRel,
        );

        self.next.store_unwrapped(
            AtomicMiniHeapId::new(new as *const _ as *mut ()),
            Ordering::AcqRel,
        );
    }

    pub fn remove(&self, mut list_head: *mut ()) {
        unsafe {
            if let Some(prev_id) = &self.prev.load(Ordering::AcqRel) {
                let mut mh = self
                    .next
                    .load_unwrapped(Ordering::AcqRel)
                    .load(Ordering::AcqRel)
                    .cast::<MiniHeap>()
                    .as_mut()
                    .unwrap();

                let free_list = mh.free_list.borrow();
                let list_head = list_head as *mut ListEntry;

                let prev = if prev_id.is_head() {
                    list_head.as_ref().unwrap()
                } else {
                    &free_list
                };

                let next_id = self.next.load(Ordering::AcqRel).unwrap();

                let next = if next_id.is_head() {
                    list_head.as_ref().unwrap()
                } else {
                    &free_list
                };

                prev.next.store_unwrapped(next_id, Ordering::AcqRel);
                prev.prev
                    .store_unwrapped((*prev_id).clone(), Ordering::AcqRel);

                self.prev.store_none(Ordering::AcqRel);
                self.next.store_none(Ordering::AcqRel);
            }
        }
    }

    fn new_from(&self) -> Self {
        unsafe {
            Self {
                prev: AtomicOption::new(self.prev.load(Ordering::AcqRel).unwrap()),
                next: AtomicOption::new(self.next.load(Ordering::AcqRel).unwrap()),
            }
        }
    }
}

pub trait Listable: PartialEq + Sized {
    fn get_free_list(&self) -> ListEntry;
    fn set_free_list(&mut self, free_list: ListEntry);
    fn get_free_list_id(&self) -> FreeListId;
    fn set_free_list_id(&mut self, free_list: FreeListId);
    fn is_large_alloc(&self) -> bool;
}
