pub mod dynarray;

use core::{slice, ops::Deref};

use crate::one_way_mmap_heap::OneWayMmapHeap;
use crate::utils::strcat;

pub struct String {
    vec: *mut u8,
    len: usize,
}

impl String {
    pub fn new(data: *mut u8, len: usize) -> Self {
        String { vec: data, len }
    }

    pub fn push_parts(&mut self, parts: &[&str]) -> Self {
        let dest = self.vec as *mut libc::c_char;
        let vec = parts.iter().fold(dest, |dest, part| {
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
