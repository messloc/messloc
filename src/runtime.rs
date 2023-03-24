use core::{
    alloc::Layout,
    cell::{Ref, RefCell, RefMut},
    ffi::c_int,
    mem::{size_of, MaybeUninit},
    ops::Deref,
    ptr::{addr_of_mut, null, NonNull},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use spin::{Mutex, MutexGuard};

use libc::{pthread_attr_t, pthread_t, sigset_t, SIG_BLOCK};

use crate::{
    cheap_heap::CheapHeap,
    comparatomic::Comparatomic,
    fake_std::Arc,
    global_heap::GlobalHeap,
    list_entry::ListEntry,
    meshable_arena::MeshableArena,
    mini_heap::{MiniHeap, MiniHeapId},
    one_way_mmap_heap::{Heap, OneWayMmapHeap},
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
}

pub struct Messloc(pub Mutex<FastWalkTime>);

impl Messloc {
    #[must_use]
    pub fn init() -> Self {
        Self(Mutex::new(FastWalkTime {
            pid: 0,
            signal_fd: 0,
            global_heap: GlobalHeap::init(),
        }))
    }

    pub fn update_pid(&mut self) {
        todo!();
        // self.pid = id();
    }

    #[allow(clippy::missing_safety_doc)]
    #[must_use]
    pub unsafe fn allocate(&self, layout: Layout) -> *mut u8 {
        dbg!("allocating");
        let mut heap = &mut self.0.lock().global_heap;
        heap.malloc(layout.size()) as *mut u8
    }

    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn deallocate(&self, ptr: *mut u8, layout: Layout) {
        self.0
            .lock()
            .global_heap
            .free(ptr as *mut (), layout.size());
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

impl Drop for Messloc {
    fn drop(&mut self) {}
}

pub struct StartThreadArgs {
    start_routine: fn(*mut ()) -> *mut (),
    args: *mut (),
}

#[allow(clippy::type_complexity)]
pub struct FreeList(pub [[(ListEntry, Comparatomic<AtomicU64>); NUM_BINS]; 3]);

unsafe impl Send for FreeList {}

impl FreeList {
    pub fn init() -> Self {
        let free_list = core::array::from_fn(|_| {
            core::array::from_fn(|_| {
                (
                    ListEntry::new(MiniHeapId::None, MiniHeapId::None),
                    Comparatomic::new(0u64),
                )
            })
        });

        FreeList(free_list)
    }

    pub fn alloc_new() -> *mut Self {
        let alloc = unsafe {
            OneWayMmapHeap.malloc(core::mem::size_of::<Self>())
                as *mut (ListEntry, Comparatomic<AtomicU64>)
        };

        (0..NUM_BINS * 3).for_each(|bin| unsafe {
            let addr = alloc.add(bin) as *mut (ListEntry, Comparatomic<AtomicU64>);
            addr.write((
                ListEntry::new(MiniHeapId::None, MiniHeapId::None),
                Comparatomic::new(0),
            ));
        });

        alloc.cast()
    }
}
