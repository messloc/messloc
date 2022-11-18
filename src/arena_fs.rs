use crate::utils::{fcntl, ftruncate, mkstemp, unlink};
use libc::c_char;
use std::fs::create_dir_all;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::id;
use std::ptr::null_mut;

const TMP_DIR: &str = "/tmp/";

pub fn open_shm_span_file(size: usize) -> i32 {
    let span_dir = {
        let mut span_dir = open_span_dir().unwrap();
        // this is required for mkstemp
        span_dir.push("XXXXXX");
        span_dir
    };
    let path = span_dir.as_os_str().as_bytes().as_ptr() as *const c_char as *mut c_char;

    unsafe {
        let fd = mkstemp(path).unwrap();
        // unlink(path).unwrap();
        ftruncate(fd, size).unwrap();
        let _ = fcntl(fd);

        fd
    }
}

fn open_span_dir() -> Option<PathBuf> {
    let pid = id();
    let mut i = 1;
    loop {
        let path = PathBuf::from(TMP_DIR);
        let path = path.join(format!("alloc-mesh-{pid}.{i}"));
        if create_dir_all(path.clone()).is_ok() {
            return Some(path);
        } else if i >= 1024 {
            break;
        } else {
            i += 1;
        }
    }
    None
}
