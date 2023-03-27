use crate::one_way_mmap_heap::OneWayMmapHeap;
use crate::{
    fake_std::dynarray::DynArray,
    mini_heap::MiniHeap,
    PAGE_SIZE,
};
use crate::NUM_BINS;
use core::ptr::null_mut;
pub type Page = [u8; PAGE_SIZE];

pub struct MeshableArena {
    pub(crate) arena_begin: *mut (),
    pub mini_heaps: DynArray<MiniHeap, NUM_BINS>,
}

unsafe impl Sync for MeshableArena {}
unsafe impl Send for MeshableArena {}

impl MeshableArena {
    pub fn init() -> Self {
        // TODO: check if meshing enabled
        Self {
            arena_begin: null_mut(),
            mini_heaps: DynArray::<MiniHeap, NUM_BINS>::create(),
        }
    }
        

    ///# Safety
    /// Unsafe
    ///
    pub unsafe fn generate_mini_heap(&mut self, alloc: *mut (), bytes: usize) -> *mut MiniHeap {
        let mini_heaps = self.mini_heaps.as_mut_slice().as_mut().unwrap();
        let empty = mini_heaps.iter().position(|x| x.is_none());
        let size = core::mem::size_of::<*mut MiniHeap>();
        let new_heap = unsafe { OneWayMmapHeap.malloc(size) as *mut MiniHeap };
        new_heap.write(MiniHeap::new(alloc, bytes));

        match empty {
            Some(pos) => {
                mini_heaps[pos] = Some(new_heap);
                self.mini_heaps.inner().add(pos).cast()
            }

            None => {
                todo!()
            }
        }
    }
    ///# Safety
    /// Unsafe
    pub unsafe fn get_mini_heap(&self, ptr: *mut ()) -> Option<*mut MiniHeap> {
        let mini_heaps = self.mini_heaps.as_slice();

        mini_heaps
            .as_ref()
            .unwrap()
            .iter()
            .find(|x| {
                if let Some(mh) = x {
                        mh.as_ref().unwrap().arena_begin == ptr.cast()
                } else {
                    false
                }
            })
            .map(|x| x as *const _ as *mut MiniHeap)
    }
}
    
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_mini_heap() {
        let mut arena = MeshableArena::init();
        unsafe { arena.generate_mini_heap(null_mut(), 0) };
        unsafe { arena.generate_mini_heap(null_mut(), 0) };
    }
}
