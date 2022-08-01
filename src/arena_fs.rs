use crate::utils::{fcntl, ftruncate, mkstemp, unlink};
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::process::id;

const TMP_DIR: &str = "/tmp";

pub fn open_shm_span_file(size: usize) -> i32 {
    let span_dir = open_span_dir().unwrap();
    // this is required for mkstemp
    span_dir.push("XXXXXX");
    unsafe {
        let fd = mkstemp(&span_dir).unwrap();
        let _ = unlink(&span_dir).unwrap();
        let _ = ftruncate(fd, size).unwrap();
        let _ = fcntl(fd);
        fd
    }
}

fn open_span_dir() -> Option<PathBuf> {
    let pid = id();
    let mut i = 1;
    loop {
        let mut path = PathBuf::from(TMP_DIR);
        path.join(format!("alloc-mesh-{pid}.{i}"));
        if create_dir_all(path).is_ok() {
            return Some(path);
        } else if i >= 1024 {
            break;
        } else {
            i += 1;
        }
    }
    None
}
