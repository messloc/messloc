use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};

#[derive(Default)]
pub struct Comparatomic<T: Atomic>(T);

impl<T> Comparatomic<T>
where
    T: Atomic,
{
    pub fn new(input: T::Innermost) -> Comparatomic<T> {
        Comparatomic(T::make(input))
    }

    pub fn inner(&self) -> &T {
        &self.0
    }

    pub fn into_inner(self) -> T {
        self.0
    }

    pub fn load(&self, ordering: Ordering) -> T::Innermost {
        self.0.load_value(ordering)
    }

    pub fn store(&self, value: T::Innermost, ordering: Ordering) {
        self.0.store_value(value, ordering)
    }
}

pub trait Atomic: Sized {
    type Innermost: PartialEq + std::fmt::Debug + Copy;
    fn make(input: Self::Innermost) -> Self;
    fn cas(
        &self,
        current: Self::Innermost,
        new: Self::Innermost,
        success: Ordering,
        failure: Ordering,
    ) -> Result<Self::Innermost, Self::Innermost>;

    fn load_value(&self, ordering: Ordering) -> Self::Innermost;
    fn store_value(&self, value: Self::Innermost, ordering: Ordering);
}
impl<T> PartialEq<Comparatomic<T>> for Comparatomic<T>
where
    T: Atomic,
{
    fn eq(&self, other: &Comparatomic<T>) -> bool {
        let left = self.0.load_value(Ordering::Acquire);
        let oth = other.0.load_value(Ordering::Acquire);
        let rhs = other
            .0
            .cas(oth, oth, Ordering::Acquire, Ordering::Relaxed)
            .unwrap();
        let lhs = self
            .0
            .cas(left, left, Ordering::Acquire, Ordering::Relaxed)
            .unwrap();
        rhs == lhs
    }
}

impl Atomic for AtomicU64 {
    type Innermost = u64;
    fn make(input: Self::Innermost) -> AtomicU64 {
        AtomicU64::new(input)
    }

    fn cas(
        &self,
        current: Self::Innermost,
        new: Self::Innermost,
        success: Ordering,
        failure: Ordering,
    ) -> Result<u64, u64> {
        self.compare_exchange(current, new, success, failure)
    }

    fn load_value(&self, ordering: Ordering) -> u64 {
        self.load(ordering)
    }

    fn store_value(&self, value: Self::Innermost, ordering: Ordering) {
        self.store_value(value, ordering);
    }
}

impl Atomic for AtomicU32 {
    type Innermost = u32;
    fn make(input: Self::Innermost) -> AtomicU32 {
        AtomicU32::new(input)
    }

    fn cas(
        &self,
        current: Self::Innermost,
        new: Self::Innermost,
        success: Ordering,
        failure: Ordering,
    ) -> Result<u32, u32> {
        self.compare_exchange(current, new, success, failure)
    }

    fn load_value(&self, ordering: Ordering) -> u32 {
        self.load(ordering)
    }

    fn store_value(&self, value: Self::Innermost, ordering: Ordering) {
        self.store_value(value, ordering);
    }
}
impl<T> Atomic for AtomicPtr<T> {
    type Innermost = *mut T;
    fn make(input: Self::Innermost) -> AtomicPtr<T> {
        AtomicPtr::new(input)
    }

    fn cas(
        &self,
        current: Self::Innermost,
        new: Self::Innermost,
        success: Ordering,
        failure: Ordering,
    ) -> Result<*mut T, *mut T> {
        self.compare_exchange(current, new, success, failure)
    }

    fn load_value(&self, ordering: Ordering) -> *mut T {
        self.load(ordering)
    }

    fn store_value(&self, value: Self::Innermost, ordering: Ordering) {
        self.store_value(value, ordering);
    }
}
