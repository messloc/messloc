use libc::{munmap, MAP_ANONYMOUS, MAP_PRIVATE};

use crate::one_way_mmap_heap::{Heap, OneWayMmapHeap};

#[derive(Default)]
pub struct MmapHeap {
    map: arrayvec::ArrayVec<(*mut (), usize), 1024>,
}

impl Heap for MmapHeap {
    type PointerType = *mut ();
    type MallocType = ();

    unsafe fn map(&mut self, size: usize, flags: libc::c_int, fd: libc::c_int) -> *mut () {
        todo!()
    }

    unsafe fn malloc(&mut self, size: usize) -> *mut () {
        let ptr = OneWayMmapHeap.map(size, MAP_PRIVATE | MAP_ANONYMOUS, -1);
        self.map.push((ptr, size));
        ptr
    }

    unsafe fn grow<T>(&mut self, src: *mut T, old: usize, new: usize) -> *mut T { 
        todo!()
    }

    unsafe fn get_size(&mut self, ptr: *mut ()) -> usize {
        self.map.iter().find(|(p, _)| *p == ptr).unwrap().1
    }

    // TODO: use layout in this method to let us not need to lookup the size
    unsafe fn free(&mut self, ptr: *mut ()) {
        let (i, _) = self
            .map
            .iter()
            .enumerate()
            .find(|(_, (p, _))| *p == ptr)
            .unwrap();
        let (_, size) = self.map.swap_remove(i);
        munmap(ptr.cast(), size);
    }
}

// // MmapHeap extends OneWayMmapHeap to track allocated address space
// // and will free memory with calls to munmap.
// class MmapHeap : public OneWayMmapHeap {
// private:
//   DISALLOW_COPY_AND_ASSIGN(MmapHeap);
//   typedef OneWayMmapHeap SuperHeap;

// public:
//   enum { Alignment = MmapWrapper::Alignment };

//   MmapHeap() : SuperHeap() {
//   }

//   inline bool inBounds(void *ptr) const {
//     auto entry = _vmaMap.find(ptr);
//     if (unlikely(entry == _vmaMap.end())) {
//       return false;
//     }
//     // FIXME: this isn't right -- we want inclusion not exact match
//     return true;
//   }

//   inline void free(void *ptr) {
//   }

//   // return the sum of the sizes of all large allocations
//   size_t arenaSize() const {
//     size_t sz = 0;
//     for (auto it = _vmaMap.begin(); it != _vmaMap.end(); it++) {
//       sz += it->second;
//     }
//     return sz;
//   }

// protected:
//   internal::unordered_map<void *, size_t> _vmaMap{};
// };
// }  // namespace mesh

// #endif  // MESH_MESH_MMAP_H
