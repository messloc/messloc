use libc::{MADV_DONTNEED, c_void, PROT_READ, FALLOC_FL_PUNCH_HOLE, FALLOC_FL_KEEP_SIZE, MAP_FIXED, PROT_WRITE, c_char, F_SETFD};
use std::io::{Error, Result};
use std::os::unix::prelude::OsStrExt;
use std::path::Path;
use crate::MAP_SHARED;

pub unsafe fn madvise(ptr: *mut c_void, size: usize) -> Result<()> {
   OutputWrapper(libc::madvise(ptr, size, MADV_DONTNEED)).into()
    
}

pub unsafe fn mprotect(addr: *mut c_void, size: usize) -> Result<()> {
    OutputWrapper(libc::mprotect(addr, size, PROT_READ)).into()

}

pub unsafe fn mmap(remove: *mut c_void, fd: i32, size: usize, offset: usize) -> Result<*mut c_void> {
    let ptr =libc::mmap(remove, size, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_FIXED, fd, i64::try_from(offset).unwrap() );

    if ptr == libc::MAP_FAILED {
        Err(Error::last_os_error())
    } else {
        Ok(ptr)
    } 
}

pub unsafe fn mkstemp(file_path: &Path) -> Result<i32> {
    let res = libc::mkstemp(file_path.as_os_str().as_bytes().as_mut_ptr() as *mut c_char);

    if res >= 0 { 
        Ok(res)
    } else {
        Err(Error::last_os_error())
    }

}

pub unsafe fn unlink(file_path: &Path) -> Result<()> {
   OutputWrapper(libc::unlink(file_path.as_os_str().as_bytes().as_mut_ptr() as *mut c_char)).into()
}


pub unsafe fn fallocate(fd: i32, offset: usize, len: usize) -> Result<()>{
    OutputWrapper(libc::fallocate(fd, FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE, i64::try_from(offset).unwrap(), i64::try_from(len).unwrap())).into()
} 

pub unsafe fn ftruncate(fd: i32, len: usize) -> Result<()> {
    OutputWrapper(libc::ftruncate(fd, i64::try_from(len).unwrap())).into()
}

pub unsafe fn fcntl(fd: i32) -> Result<()> {
   OutputWrapper(libc::fcntl(fd, F_SETFD)).into()
}

struct OutputWrapper(pub i32);

impl From<OutputWrapper> for Result<()> {
    fn from(output: OutputWrapper) -> Self {
        if output.0 == 0 {
            Ok(())
        } else {
            Err(Error::last_os_error())
        }
            
}
}

