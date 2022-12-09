use core::marker::PhantomData;
use core::ptr::addr_of_mut;
use core::sync::atomic::Ordering;

use crate::fake_std::Arc;
use crate::mini_heap::FreeListId;
use crate::mini_heap::{MiniHeap, MiniHeapId};
use crate::runtime::Messloc;
use spin::mutex::Mutex;

#[derive(Default)]
pub struct ListEntry {
    pub prev: MiniHeapId,
    pub next: MiniHeapId,
}

impl ListEntry {
    pub const fn new(prev: MiniHeapId, next: MiniHeapId) -> Self {
        Self { prev, next }
    }

    pub fn add(
        &mut self,
        list_head: *mut Self,
        list_id: u32,
        self_id: MiniHeapId,
        mut new: *mut (),
    ) {
        let new = unsafe { new.cast::<MiniHeap>().as_mut().unwrap() };
        let old_id = new.get_free_list_id();
        assert!(!new.is_large_alloc());

        new.free_list.remove(list_head);
        new.set_free_list_id(FreeListId::from_integer(list_id));

        match &self.prev {
            MiniHeapId::Head => {
                self.next = MiniHeapId::HeapPointer(new as *const _ as *mut MiniHeap);
            }
            id @ MiniHeapId::HeapPointer(p) => {
                let mut prev_list = unsafe { &mut p.as_mut().unwrap().free_list };
                prev_list.next = MiniHeapId::HeapPointer(new as *const _ as *mut MiniHeap);

                new.set_free_list(Self::new(MiniHeapId::HeapPointer(*p), self_id));
                self.prev = MiniHeapId::HeapPointer(new as *const _ as *mut MiniHeap);
                self.next = MiniHeapId::HeapPointer(new as *const _ as *mut MiniHeap);
            }
            _ => todo!(),
        }
    }

    pub fn remove(&mut self, mut list_head: *mut Self) {
        match (&self.prev, &self.next) {
            (prev @ &MiniHeapId::HeapPointer(p), next @ MiniHeapId::HeapPointer(q)) => unsafe {
                addr_of_mut!((*p).free_list.next).write(MiniHeapId::HeapPointer(p));
                addr_of_mut!((*p).free_list.prev).write(MiniHeapId::HeapPointer(p));
            },

            (prev @ &MiniHeapId::HeapPointer(p), MiniHeapId::Head) => unsafe {
                addr_of_mut!((*p).free_list.next)
                    .write(MiniHeapId::HeapPointer(list_head as *mut MiniHeap));
                addr_of_mut!((*p).free_list.prev).write(MiniHeapId::HeapPointer(p));
            },

            (MiniHeapId::Head, prev @ &MiniHeapId::HeapPointer(p)) => unsafe {
                addr_of_mut!((*p).free_list.next).write(MiniHeapId::HeapPointer(p));
                addr_of_mut!((*p).free_list.prev)
                    .write(MiniHeapId::HeapPointer(list_head as *mut MiniHeap));
            },

            (MiniHeapId::Head, MiniHeapId::Head) => {
                todo!()
            }

            _ => unreachable!(),
        }

        self.prev = MiniHeapId::None;
        self.next = MiniHeapId::None;
    }
}

pub trait Listable: PartialEq + Sized {
    fn get_free_list(&self) -> ListEntry;
    fn set_free_list(&mut self, free_list: ListEntry);
    fn get_free_list_id(&self) -> FreeListId;
    fn set_free_list_id(&mut self, free_list: FreeListId);
    fn is_large_alloc(&self) -> bool;
}
