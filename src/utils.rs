#![allow(unused)]

use core::ffi::c_int;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use libc::{
    c_char, c_void, pthread_attr_t, pthread_t, signalfd_siginfo, sigset_t, size_t,
    FALLOC_FL_KEEP_SIZE, FALLOC_FL_PUNCH_HOLE, F_SETFD, MADV_DONTNEED, PROT_READ, PROT_WRITE,
    SIGRTMIN,
};

use std::io::Error;

// temporary result type till no_std shenanigans are fixed
pub type Result<T> = core::result::Result<T, std::io::Error>;

pub fn sigdump() -> i32 {
    libc::SIGRTMIN() + 8
}

pub unsafe fn madvise(ptr: *mut c_void, size: usize) -> Result<()> {
    OutputWrapper(libc::madvise(ptr, size, MADV_DONTNEED)).into()
}

pub unsafe fn mprotect_read(addr: *mut c_void, size: usize) -> Result<()> {
    OutputWrapper(libc::mprotect(addr, size, PROT_READ)).into()
}

pub unsafe fn mprotect_write(addr: *mut c_void, size: usize) -> Result<()> {
    OutputWrapper(libc::mprotect(addr, size, PROT_READ | PROT_WRITE)).into()
}

#[allow(clippy::unnecessary_wraps)]
pub unsafe fn mmap(addr: *mut c_void, fd: i32, size: usize, offset: usize) -> Result<*mut c_void> {
    //TODO: replace it with libc's mmap when issue has been figured out
    Ok(memmap2::MmapRaw::map_raw(fd).unwrap().as_mut_ptr() as *mut c_void)
    /*
    let ptr = libc::mmap(
        addr,
        size,
        PROT_READ | PROT_WRITE,
        MAP_FIXED | MAP_SHARED,
        fd,
        i64::try_from(offset).unwrap(),
    );

    if ptr == libc::MAP_FAILED {
        Err(Error::last_os_error())
    } else {
        Ok(ptr)
    }
    */
}

pub unsafe fn mkstemp(file_path: *mut c_char) -> Result<i32> {
    let res = libc::mkstemp(file_path);

    if res >= 0 {
        Ok(res)
    } else {
        Err(Error::last_os_error())
    }
}

pub unsafe fn make_dir_if_not_exists(file_path: *mut c_char) -> Result<Option<()>> {
    let buf = core::mem::transmute::<_, libc::stat>([0; 36]);

    if libc::stat(file_path, &buf as *const _ as *mut libc::stat) == 0 {
        Ok(None)
    } else {
        Ok(Some(mkdir(file_path)?))
    }
}

pub unsafe fn mkdir(file_path: *mut c_char) -> Result<()> {
    OutputWrapper(libc::mkdir(file_path, libc::S_IRUSR | libc::S_IWUSR)).into()
}

pub unsafe fn strcat(dest: *mut c_char, src: *const c_char, len: usize) -> *mut c_char {
    libc::strncat(dest, src, len as size_t)
}

pub unsafe fn unlink(file_path: *mut c_char) -> Result<()> {
    OutputWrapper(libc::unlink(file_path)).into()
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

pub fn get_pid() -> u32 {
    unsafe { libc::getpid() as u32 }
}

pub unsafe fn ftruncate(fd: i32, len: usize) -> Result<()> {
    OutputWrapper(libc::ftruncate(fd, i64::try_from(len).unwrap())).into()
}

pub unsafe fn fcntl(fd: i32) -> Result<()> {
    OutputWrapper(libc::fcntl(fd, F_SETFD)).into()
}

pub unsafe fn pipe(mut fork_pipe: [i32; 2]) -> Result<()> {
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
        unreachable!()
    }
}

pub unsafe fn wait_till_memory_ready(fd: i32) {
    let mut buf = [0u8; 4];
    loop {
        if read(fd, buf.as_mut_ptr() as *mut c_void, 4).is_ok() {
            break;
        }
    }
}

pub unsafe fn create_signal_mask() -> Option<*mut sigset_t> {
    let mask = [0u64; 16].as_mut_ptr() as *mut sigset_t;
    libc::sigemptyset(mask);
    let result = libc::sigaddset(mask, SIGRTMIN() + 8);
    (result == 0).then_some(mask)
}

pub unsafe fn sig_proc_mask(how: c_int, set: *mut sigset_t, old_set: *mut sigset_t) -> Result<()> {
    OutputWrapper(libc::sigprocmask(how, set, old_set)).into()
}

pub unsafe fn signalfd_siginfo() -> signalfd_siginfo {
    let buffer = [0; 32];
    core::mem::transmute::<_, signalfd_siginfo>(buffer)
}

pub unsafe fn new_signal_fd(mask: *mut sigset_t) -> Result<c_int> {
    let result = libc::signalfd(-1i32, mask, 0);
    if result > 0 {
        Ok(result)
    } else {
        unreachable!()
    }
}

pub unsafe fn pthread_create(
    thread: pthread_t,
    attr: &[pthread_attr_t],
    start_routine: fn(*mut c_void) -> *mut c_void,
    args: *mut (),
) -> Result<()> {
    let start_routine =
        core::mem::transmute::<_, extern "C" fn(*mut c_void) -> *mut c_void>(start_routine);
    OutputWrapper(libc::pthread_create(
        thread as *mut pthread_t,
        attr.as_ptr(),
        start_routine,
        args as *mut c_void,
    ))
    .into()
}

pub unsafe fn pthread_exit(value: *mut c_void) -> ! {
    libc::pthread_exit(value)
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

pub unsafe fn fstat(fildes: i32, buf: &mut MaybeUninit<Stat>) -> Result<()> {
    // FIXME:: check if this is UB or not
    OutputWrapper(libc::fstat(fildes, addr_of_mut!(buf.assume_init_mut().0))).into()
}

pub fn popcountl(bit: u64) -> u64 {
    todo!()
}

pub fn ffsll(bits: u64) -> u64 {
    todo!()
}

pub fn builtin_prefetch(memory: *mut ()) {
    todo!()
}

pub const fn stlog(input: usize) -> usize {
    match input {
        1 => 0,
        2 => 1,
        _ => stlog(input / 2) + 1,
    }
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
