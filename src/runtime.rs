use std::{
    process::id,
    sync::{Arc, Mutex, MutexGuard, PoisonError, atomic::Ordering}, ffi::c_int, ptr::null, mem::{size_of, MaybeUninit}, time::Duration, thread::yield_now,
};

use libc::{sigset_t, pthread_t, pthread_attr_t, SIG_BLOCK};

use crate::{
    global_heap::GlobalHeap,
    cheap_heap::CheapHeap,
    one_way_mmap_heap::Heap,
    list_entry::ListEntry,
    mini_heap::{AtomicMiniHeapId, MiniHeap},
    rng::Rng,
    splits::MergeSetWithSplits,
    thread_local_heap::ThreadLocalHeap,
    utils::{madvise, create_signal_mask, sig_proc_mask, new_signal_fd, pthread_create, pthread_exit, read, signalfd_siginfo, SIGDUMP},
    MAX_MERGE_SETS, MAX_SPLIT_LIST_SIZE, NUM_BINS, MESHES_PER_MAP, 
};

struct FastWalkTime<'a> {
    pid: u32,
    pub global_heap: GlobalHeap<'a>,
    pub merge_set: MergeSetWithSplits<'a>,
    pub free_lists: [[(ListEntry<'a>, u64); NUM_BINS]; 3],
    pub rng: Rng,
    pub signal_fd: i32,
    pub thread_local_heap: &'a ThreadLocalHeap<'a>,
}

impl<'a> FastWalkTime<'a> {
pub fn init(&self) {
       //TODO: consider whether to init handlers or not 
       //
      self.create_signal_fd();
      self.install_segfault_handler();
      self.init_max_map_count();

      let mesh_period = std::env::var("MESH_PERIOD_MS").unwrap();
      let period = mesh_period.parse().unwrap();
      self.global_heap.set_mesh_period_ms(Duration::from_millis(period));

      let background_thread = std::env::var("MESH_BACKGROUND_THREAD").unwrap();

      if let Ok(1u8) = background_thread.parse() {
            self.start_background_thread();
      }
    }


pub fn create_signal_fd(&self) {
        unsafe {
        let mask = create_signal_mask().unwrap();
        let result = self.sig_proc_mask(mask);
        self.signal_fd = new_signal_fd(mask).unwrap(); 
        }
    }

pub fn install_segfault_handler(&self) {
    //TODO: consider if we need a segfault handler or not
      todo!()  
}

pub fn create_thread(&self, thread: libc::pthread_t, attr: &[pthread_attr_t], start_routine: fn(*mut libc::c_void) -> *mut libc::c_void, arg: *mut ()) {
   unsafe { pthread_create(thread, attr, start_routine, arg) };

}

pub fn start_thread(&self, args: StartThreadArgs) -> *mut (){ 
   self.install_segfault_handler();
   (args.start_routine)(args.args)
}

pub fn exit_thread(&self, ret_val: *mut ()) {
    let heap = self.thread_local_heap;
    heap.release_all();
    unsafe { pthread_exit(ret_val as *mut libc::c_void) };

}

   pub unsafe fn sig_proc_mask(&self, mask: *mut sigset_t) {
        //TODO: add signal mutex if needed
        sig_proc_mask(SIG_BLOCK, mask, null::<sigset_t>() as *mut _).unwrap();
    }

    pub fn init_max_map_count(&self) {
        // TODO: this should run only on linux
        
        let buf = std::fs::read_to_string("/proc/sys/vm/max_map_count").unwrap();
        let map_count: usize = buf.parse().unwrap();
        
        let mesh_count = (MESHES_PER_MAP * map_count as f64).trunc();
        self.global_heap.guarded.lock().unwrap().arena.set_max_mesh_count(map_count);
     }

    pub fn start_background_thread(&self) {
        std::thread::spawn(|| {
            //TODO:: linux-gate this 
            let buf = unsafe { signalfd_siginfo() }; 
            let s = unsafe { read(self.signal_fd, &buf as *const _ as *mut libc::c_void, std::mem::size_of::<libc::signalfd_siginfo>()).unwrap() };
            if buf.ssi_signo == SIGDUMP as u32 {
                self.global_heap.dump_strings();
            }

            //TODO:: add a retry check somehow and a counter if needed
            yield_now();
        });

    }
}

pub struct Runtime<'a>(Arc<Mutex<FastWalkTime<'a>>>);

impl<'a> Runtime<'a> {

    pub fn init(&self) {
       //TODO: consider whether to init handlers or not 
       //
    }

    pub fn share(&self) -> Self {
        Runtime(Arc::clone(&self.0))
    }

    pub fn update_pid(&mut self) {
        todo!();
        // self.pid = id();
    }

    pub fn lock(
        &self,
    ) -> Result<MutexGuard<'_, FastWalkTime<'_>>, PoisonError<MutexGuard<'_, FastWalkTime<'_>>>>
    {
        self.0.lock()
    }

    pub fn global_heap(&self) -> &GlobalHeap<'a> {
        &(&*self.0.lock().unwrap()).global_heap
    }

    }
    
impl PartialEq<Self> for Runtime<'_> {
    fn eq(&self, rhs: &Self) -> bool {
        // This is a hack to ensure that partial eq can be implemented on other types
        // Runtime in a singleton instance and hence can be ignored from the partialeq
        // checks

        true
    }
}

struct StartThreadArgs {
    start_routine: fn(*mut ()) -> *mut(),
    args: *mut (),
}
