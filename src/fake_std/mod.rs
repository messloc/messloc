pub mod dynarray;

use core::ops::{Deref, DerefMut};
use core::ptr::addr_of;
use core::slice;
use core::{marker::PhantomData, ptr::NonNull, sync::atomic::AtomicUsize};

use crate::one_way_mmap_heap::{Heap, OneWayMmapHeap};
use crate::utils::strcat;

pub struct Arc<T: ?Sized> {
    ptr: NonNull<ArcInner<T>>,
    phantom: PhantomData<ArcInner<T>>,
}

pub struct ArcInner<T: ?Sized> {
    strong: AtomicUsize,
    weak: AtomicUsize,
    data: T,
}

impl<T> Arc<T> {
    pub fn new(data: T) -> Arc<T> {
        unsafe {
            let kasten = OneWayMmapHeap.malloc(core::mem::size_of::<Self>()) as *mut ArcInner<T>;

            kasten.write(ArcInner {
                strong: AtomicUsize::new(1),
                weak: AtomicUsize::new(1),
                data,
            });

            Arc {
                ptr: NonNull::new_unchecked(kasten),
                phantom: PhantomData,
            }
        }
    }

    pub fn inner(&self) -> &ArcInner<T> {
        unsafe { self.ptr.as_ref() }
    }
}

impl<T> Deref for Arc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner().data
    }
}

unsafe impl<T: ?Sized + Sync + Send> Send for Arc<T> {}
unsafe impl<T: ?Sized + Sync + Send> Sync for Arc<T> {}

impl<T: ?Sized> Drop for Arc<T> {
    fn drop(&mut self) {}
}

pub struct String {
    vec: *mut u8,
    len: usize,
}

impl String {
    pub fn new(data: *mut u8, len: usize) -> Self {
        String { vec: data, len }
    }

    pub fn from(data: &str) -> Self {
        String::new(data.as_ptr() as *mut u8, data.len())
    }

    pub fn as_str(&self) -> &str {
        self
    }

    pub fn push_parts(&mut self, parts: &[&str]) -> Self {
        let mut dest = self.vec as *mut libc::c_char;
        let vec = parts.iter().fold(dest, |dest, mut part| {
            let catted =
                unsafe { strcat(dest, part.as_bytes() as *const _ as *mut i8, self.len()) };
            self.len += part.len();
            catted
        });

        String::new(vec.cast(), self.len())
    }
}

pub trait ToString {
    fn to_string(&self) -> String;
}

impl ToString for u32 {
    fn to_string(&self) -> String {
        let buf = unsafe {
            core::ptr::slice_from_raw_parts_mut(
                OneWayMmapHeap
                    .malloc(10 * core::mem::size_of::<u8>())
                    .cast::<u8>(),
                10,
            )
            .as_mut()
            .unwrap()
        };

        let mut n = *self;
        let mut count = 0;
        while n != 0 {
            let digit = (b'0' + n as u8 % 10) as char;
            buf[count] = digit.try_into().unwrap();
            n /= 10;
            count += 1;
        }
        buf[count] = b'\0';

        String::new(buf.as_ptr() as *mut u8, 10)
    }
}

impl Deref for String {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        let slice = unsafe { slice::from_raw_parts_mut(self.vec, self.len) };
        unsafe { core::str::from_utf8_unchecked(slice) }
    }
}
