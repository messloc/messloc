use lazy_static::__Deref;

use crate::comparatomic::{Atomic, Comparatomic};
use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::ptr::addr_of_mut;
use std::ptr::write;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::{marker::PhantomData, sync::atomic::AtomicU32};

pub struct AtomicEnum<T, A = AtomicU8>
where
    A: Atomic,
{
    union: T,
    discriminant: Comparatomic<A>,
}

pub union UnionisedOption<T> {
    some: ManuallyDrop<T>,
    none: (),
}

pub struct AtomicOption<T>(RefCell<AtomicEnum<UnionisedOption<T>, AtomicBool>>);

impl<T> AtomicOption<T> {
    pub fn new(item: T) -> Self {
        AtomicOption(RefCell::new(AtomicEnum {
            union: UnionisedOption {
                some: ManuallyDrop::new(item),
            },
            discriminant: Comparatomic::new(true),
        }))
    }

    pub fn inner(&self) -> &RefCell<AtomicEnum<UnionisedOption<T>, AtomicBool>> {
        &self.0
    }
    pub fn store_unwrapped(&self, value: T, ordering: Ordering) {
        self.0.borrow_mut().union.some = ManuallyDrop::new(value);
        self.0
            .borrow_mut()
            .discriminant
            .store(true, Ordering::AcqRel);
    }

    pub unsafe fn load_unwrapped(&self, ordering: Ordering) -> T {
        if (self.0.borrow_mut()).discriminant.load(ordering) {
            let borrow = self.0.as_ptr();
            ManuallyDrop::take(&mut addr_of_mut!((*borrow).union.some).read())
        } else {
            unreachable!()
        }
    }

    pub fn store_none(&self, ordering: Ordering) {
        self.0.borrow_mut().union.none = ();
        self.0
            .borrow_mut()
            .discriminant
            .store(false, Ordering::AcqRel);
    }

    pub fn store(&self, value: Option<T>, ordering: Ordering) {
        let discriminant = value.is_some();
        if let Some(val) = value {
            unsafe { self.0.borrow_mut().union.some = (ManuallyDrop::new(val)) }
        }
        self.0
            .borrow_mut()
            .discriminant
            .store(discriminant, Ordering::AcqRel);
    }

    pub unsafe fn load(&self, ordering: Ordering) -> Option<T> {
        if (self.0.borrow_mut()).discriminant.load(ordering) {
            let borrow = self.0.as_ptr();
            Some(ManuallyDrop::take(
                &mut addr_of_mut!((*borrow).union.some).read(),
            ))
        } else {
            None
        }
    }
}

impl<T> Default for AtomicOption<T> {
    fn default() -> Self {
        AtomicOption(RefCell::new(AtomicEnum {
            union: UnionisedOption { none: () },
            discriminant: Comparatomic::new(false),
        }))
    }
}

impl<T> PartialEq<Option<T>> for AtomicOption<T>
where
    T: PartialEq,
{
    fn eq(&self, other: &Option<T>) -> bool {
        unsafe { self.load(Ordering::AcqRel) == *other }
    }
}
