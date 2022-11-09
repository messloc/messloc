use std::{
    alloc::Layout,
    cell::{Ref, RefCell, RefMut},
    ffi::c_int,
    mem::{size_of, MaybeUninit},
    ops::Deref,
    process::id,
    ptr::{addr_of_mut, null, NonNull},
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard, PoisonError,
    },
    thread::{current, yield_now},
    time::Duration,
};

use libc::{pthread_attr_t, pthread_t, sigset_t, SIG_BLOCK};

use crate::{
    cheap_heap::CheapHeap,
    comparatomic::Comparatomic,
    global_heap::GlobalHeap,
    list_entry::ListEntry,
    meshable_arena::MeshableArena,
    mini_heap::{MiniHeap, MiniHeapId},
    one_way_mmap_heap::Heap,
    rng::Rng,
    splits::MergeSetWithSplits,
    utils::{
        create_signal_mask, madvise, new_signal_fd, pthread_create, pthread_exit, read,
        sig_proc_mask, sigdump, signalfd_siginfo,
    },
    MAX_MERGE_SETS, MAX_SPLIT_LIST_SIZE, MESHES_PER_MAP, NUM_BINS,
};

pub struct FastWalkTime {
    pid: u32,
    pub signal_fd: i32,
    pub global_heap: GlobalHeap,
}

impl FastWalkTime {
    pub fn init(&mut self) {
        //TODO: consider whether to init handlers or not
        //
        self.create_signal_fd();
        self.install_segfault_handler();
        self.init_max_map_count();

        let background_thread = std::env::var("MESH_BACKGROUND_THREAD").unwrap();

        if let Ok(1u8) = background_thread.parse() {
            self.start_background_thread();
        }
    }

    pub fn create_signal_fd(&mut self) {
        unsafe {
            let mask = create_signal_mask().unwrap();
            self.sig_proc_mask(mask);
            self.signal_fd = new_signal_fd(mask).unwrap();
        }
    }

    pub fn install_segfault_handler(&self) {
        //TODO: consider if we need a segfault handler or not
        todo!()
    }
    pub fn create_thread(
        &self,
        thread: libc::pthread_t,
        attr: &[pthread_attr_t],
        start_routine: fn(*mut libc::c_void) -> *mut libc::c_void,
        arg: *mut (),
    ) {
        unsafe { pthread_create(thread, attr, start_routine, arg) };
    }

    pub fn start_thread(&self, args: StartThreadArgs) -> *mut () {
        self.install_segfault_handler();
        (args.start_routine)(args.args)
    }

    /*
    pub fn exit_thread(&mut self, ret_val: *mut ()) {
        self.global_heap.release_all();
        unsafe { pthread_exit(ret_val as *mut libc::c_void) };
    }
    */

    pub unsafe fn sig_proc_mask(&self, mask: *mut sigset_t) {
        //TODO: add signal mutex if needed
        sig_proc_mask(SIG_BLOCK, mask, null::<sigset_t>() as *mut _).unwrap();
    }

    pub fn init_max_map_count(&self) {
        // TODO: this should run only on linux

        let buf = std::fs::read_to_string("/proc/sys/vm/max_map_count").unwrap();
        let map_count: usize = buf.parse().unwrap();
    }

    pub fn start_background_thread(&self) {
        let signal_fd = self.signal_fd;
        std::thread::spawn(move || {
            //TODO:: linux-gate this
            let buf = unsafe { signalfd_siginfo() };
            unsafe {
                read(
                    signal_fd,
                    &buf as *const _ as *mut libc::c_void,
                    std::mem::size_of::<libc::signalfd_siginfo>(),
                )
                .unwrap();
            }

            //TODO:: add a retry check somehow and a counter if needed
            yield_now();
        });
    }
}

pub struct Messloc(pub Arc<Mutex<FastWalkTime>>);

impl Messloc {
    #[must_use]
    pub fn init() -> Messloc {
        Messloc(Arc::new(Mutex::new(FastWalkTime {
            pid: 0,
            signal_fd: 0,
            global_heap: GlobalHeap::init(),
        })))
    }

    #[must_use]
    pub fn share(&self) -> Self {
        Self(Arc::clone(&self.0))
    }

    pub fn update_pid(&mut self) {
        todo!();
        // self.pid = id();
    }

    #[allow(clippy::missing_safety_doc)]
    #[must_use]
    pub unsafe fn allocate(&self, layout: Layout) -> *mut u8 {
        let mut heap = &mut self.0.lock().unwrap().global_heap;
        heap.malloc(layout.size()) as *mut u8
    }

    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn deallocate(&self, ptr: *mut u8, layout: Layout) {
        self.0.lock().unwrap().global_heap.free(ptr as *mut ());
    }
}

impl PartialEq<Self> for Messloc {
    fn eq(&self, rhs: &Self) -> bool {
        // This is a hack to ensure that partial eq can be implemented on other types
        // Runtime in a singleton instance and hence can be ignored from the partialeq
        // checks

        true
    }
}

pub struct StartThreadArgs {
    start_routine: fn(*mut ()) -> *mut (),
    args: *mut (),
}

#[allow(clippy::type_complexity)]
pub struct FreeList(pub [[(ListEntry, Comparatomic<AtomicU64>); 1]; 3]);

unsafe impl Send for FreeList {}

impl FreeList {
    pub fn init() -> Self {
        let free_list = std::array::from_fn(|_| {
            [(
                ListEntry::new(MiniHeapId::None, MiniHeapId::None),
                Comparatomic::new(0u64),
            )]
        });

        FreeList(free_list)
    }
}
