use core::{
    ops::BitAnd,
    sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicU8, Ordering},
};

#[derive(Default)]
pub struct Comparatomic<T: Atomic>(T);

impl<T> Comparatomic<T>
where
    T: Atomic,
{
    pub fn new(input: T::Innermost) -> Self {
        Self(T::make(input))
    }

    pub const fn inner(&self) -> &T {
        &self.0
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn into_inner(self) -> T {
        self.0
    }

    pub fn load(&self, ordering: Ordering) -> T::Innermost {
        self.0.load_value(ordering)
    }

    pub fn store(&self, value: T::Innermost, ordering: Ordering) {
        self.0.store_value(value, ordering);
    }

    pub fn fetch_add(&self, value: T::Innermost, ordering: Ordering) {
        self.0.fetch_add(value, ordering);
    }
}

pub trait Atomic: Sized {
    type Innermost: PartialEq + core::fmt::Debug + Copy;
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
    fn fetch_add(&self, value: Self::Innermost, ordering: Ordering);
}
impl<T> PartialEq<Self> for Comparatomic<T>
where
    T: Atomic,
{
    fn eq(&self, other: &Self) -> bool {
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
    fn make(input: Self::Innermost) -> Self {
        Self::new(input)
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
        self.store(value, ordering);
    }

    fn fetch_add(&self, value: Self::Innermost, ordering: Ordering) {
        self.fetch_add(value, ordering);
    }
}

impl Atomic for AtomicU32 {
    type Innermost = u32;
    fn make(input: Self::Innermost) -> Self {
        Self::new(input)
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
        self.store(value, ordering);
    }

    fn fetch_add(&self, value: Self::Innermost, ordering: Ordering) {
        self.fetch_add(value, ordering);
    }
}
impl<T> Atomic for AtomicPtr<T> {
    type Innermost = *mut T;
    fn make(input: Self::Innermost) -> Self {
        Self::new(input)
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
        self.store(value, ordering);
    }

    fn fetch_add(&self, _value: Self::Innermost, _ordering: Ordering) {
        unreachable!()
    }
}

impl Atomic for AtomicU8 {
    type Innermost = u8;
    fn make(input: Self::Innermost) -> Self {
        Self::new(input)
    }

    fn cas(
        &self,
        current: Self::Innermost,
        new: Self::Innermost,
        success: Ordering,
        failure: Ordering,
    ) -> Result<u8, u8> {
        self.compare_exchange(current, new, success, failure)
    }

    fn load_value(&self, ordering: Ordering) -> u8 {
        self.load(ordering)
    }

    fn store_value(&self, value: Self::Innermost, ordering: Ordering) {
        self.store(value, ordering);
    }

    fn fetch_add(&self, value: Self::Innermost, ordering: Ordering) {
        self.fetch_add(value, ordering);
    }
}

impl Atomic for AtomicBool {
    type Innermost = bool;
    fn make(input: Self::Innermost) -> Self {
        Self::new(input)
    }

    fn cas(
        &self,
        current: Self::Innermost,
        new: Self::Innermost,
        success: Ordering,
        failure: Ordering,
    ) -> Result<bool, bool> {
        self.compare_exchange(current, new, success, failure)
    }

    fn load_value(&self, ordering: Ordering) -> bool {
        self.load(ordering)
    }

    fn store_value(&self, value: Self::Innermost, ordering: Ordering) {
        self.store(value, ordering);
    }

    fn fetch_add(&self, _value: Self::Innermost, _ordering: Ordering) {
        // fetch_add is not applicable on atomic bool
        unreachable!()
    }
}

impl From<Comparatomic<AtomicU64>> for u64 {
    fn from(x: Comparatomic<AtomicU64>) -> Self {
        x.load(Ordering::Acquire)
    }
}

impl<T> BitAnd<&Comparatomic<T>> for &Comparatomic<T>
where
    T: Atomic,
    T::Innermost: BitAnd<T::Innermost, Output = T::Innermost>,
{
    type Output = T::Innermost;

    fn bitand(self, rhs: &Comparatomic<T>) -> Self::Output {
        self.load(Ordering::Acquire) & rhs.load(Ordering::Acquire)
    }
}
