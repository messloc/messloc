use crate::MAP_SHARED;
use libc::{
    c_char, c_void, FALLOC_FL_KEEP_SIZE, FALLOC_FL_PUNCH_HOLE, F_SETFD, MADV_DONTNEED, MAP_FIXED,
    PROT_READ, PROT_WRITE,
};
use std::io::{Error, Result};
use std::mem::MaybeUninit;
use std::os::unix::prelude::OsStrExt;
use std::path::Path;

pub unsafe fn madvise(ptr: *mut c_void, size: usize) -> Result<()> {
    OutputWrapper(libc::madvise(ptr, size, MADV_DONTNEED)).into()
}

pub unsafe fn mprotect_read(addr: *mut c_void, size: usize) -> Result<()> {
    OutputWrapper(libc::mprotect(addr, size, PROT_READ)).into()
}

pub unsafe fn mprotect_write(addr: *mut c_void, size: usize) -> Result<()> {
    OutputWrapper(libc::mprotect(addr, size, PROT_READ | PROT_WRITE)).into()
}

pub unsafe fn mmap(addr: *mut c_void, fd: i32, size: usize, offset: usize) -> Result<*mut c_void> {
    let ptr = libc::mmap(
        addr,
        size,
        PROT_READ | PROT_WRITE,
        MAP_SHARED | MAP_FIXED,
        fd,
        i64::try_from(offset).unwrap(),
    );

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
    OutputWrapper(libc::unlink(
        file_path.as_os_str().as_bytes().as_mut_ptr() as *mut c_char
    ))
    .into()
}

pub unsafe fn fallocate(fd: i32, offset: usize, len: usize) -> Result<()> {
    OutputWrapper(libc::fallocate(
        fd,
        FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE,
        i64::try_from(offset).unwrap(),
        i64::try_from(len).unwrap(),
    ))
    .into()
}

pub unsafe fn ftruncate(fd: i32, len: usize) -> Result<()> {
    OutputWrapper(libc::ftruncate(fd, i64::try_from(len).unwrap())).into()
}

pub unsafe fn fcntl(fd: i32) -> Result<()> {
    OutputWrapper(libc::fcntl(fd, F_SETFD)).into()
}

pub unsafe fn pipe(fork_pipe: [i32; 2]) -> Result<()> {
    OutputWrapper(libc::pipe(fork_pipe.as_mut_ptr())).into()
}

pub unsafe fn close(fd: i32) -> Result<()> {
    OutputWrapper(libc::close(fd)).into()
}

pub unsafe fn read(fd: i32, buf: *mut c_void, len: usize) -> Result<()> {
    let res = libc::read(fd, buf, len);

    if res >= 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

pub unsafe fn wait_till_memory_ready(fd: i32) {
    let buf = [0u8; 4];
    loop {
        if read(fd, buf.as_mut_ptr() as *mut c_void, 4).is_ok() {
            break;
        }
    }
}

type UnsafeFunction = Option<unsafe extern "C" fn()>;

pub unsafe fn pthread_atfork(
    prepare: UnsafeFunction,
    parent: UnsafeFunction,
    child: UnsafeFunction,
) -> Result<()> {
    OutputWrapper(libc::pthread_atfork(prepare, parent, child)).into()
}

#[derive(Clone)]
pub struct Stat(libc::stat);

pub unsafe fn fstat(fildes: i32, buf: &MaybeUninit<Stat>) -> Result<()> {
    // FIXME:: check if this is UB or not
    OutputWrapper(libc::fstat(
        fildes,
        &mut buf.assume_init_mut().0 as *mut libc::stat,
    ))
    .into()
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
