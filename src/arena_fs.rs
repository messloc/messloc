#![allow(unused)]

use crate::fake_std::{String, ToString};
use crate::{
    one_way_mmap_heap::OneWayMmapHeap,
    utils::{fcntl, ftruncate, get_pid, make_dir_if_not_exists, mkdir, mkstemp, unlink},
};
use core::mem::size_of_val;
use core::ptr::null_mut;
use libc::c_char;

const TMP_DIR: &str = "/tmp/";

pub fn open_shm_span_file(size: usize) -> i32 {
    let span_dir = {
        let mut span_dir = open_span_dir().unwrap();
        // this is required for mkstemp
        span_dir.push_parts(&["XXXXXX\0"])
    };
    let path = span_dir.as_ptr() as *const i8 as *mut i8;
    unsafe {
        let fd = mkstemp(path).unwrap();
        // unlink(path).unwrap();
        ftruncate(fd, size).unwrap();
        let _ = fcntl(fd);

        fd
    }
}

fn open_span_dir() -> Option<String> {
    let pid = get_pid();
    let mut count = 1;
    unsafe {
        let size = core::mem::size_of_val("/tmp/alloc_mesh-XXXXXXXXX.YYYY");
        let mut path =
            core::ptr::slice_from_raw_parts_mut(OneWayMmapHeap.malloc(size), size) as *mut [u8; 30];
        path.write(*b"/tmp/alloc-mesh-XXXXXXXXX.YYYY");
        let path = path.as_mut().unwrap();
        let dir = &path[..10];
        make_dir_if_not_exists(dir.as_ptr() as *mut i8).unwrap();
        let pid = ToString::to_string(&pid);
        let pid = pid.as_bytes();
        path.iter_mut()
            .skip_while(|x| **x != b'X')
            .enumerate()
            .take(10)
            .for_each(|(k, x)| {
                if pid[k] > b'\0' {
                    *x = pid[k];
                } else {
                    *x = b'0';
                }
            });

        loop {
            let i = ToString::to_string(&count);
            let i = i.as_bytes();
            path.iter_mut()
                .skip_while(|x| **x != b'Y')
                .enumerate()
                .take(4)
                .for_each(|(k, x)| {
                    if i[k] > b'\0' {
                        *x = pid[k];
                    } else {
                        *x = b'0';
                    }
                });
            let p = path.as_ptr() as *mut i8;
            if mkdir(p).is_ok() {
                return Some(String::new(p.cast(), path.len()));
            } else if count >= 1024 {
                break;
            }
            count += 1;
        }
    }
    None
}
