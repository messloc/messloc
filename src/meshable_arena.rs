use std::ptr::null_mut;

pub struct MeshableArena;
#[derive(Default)]
pub struct Span {
    offset: Offset,
    length: Length,
}
#[derive(Default)]
pub struct Offset(u32);
#[derive(Default)]
pub struct Length(u32);

impl MeshableArena {
    pub fn page_alloc(&self, page_count: usize, page_align: usize) -> (Span, *mut u8) {
        if page_count == 0 {
            return (Span::default(), null_mut());
        }

        (Span::default(), null_mut())
    }
}
// char *MeshableArena::pageAlloc(Span &result, size_t pageCount, size_t pageAlignment) {
//     if (pageCount == 0) {
//       return nullptr;
//     }

//     d_assert(_arenaBegin != nullptr);

//     d_assert(pageCount >= 1);
//     d_assert(pageCount < std::numeric_limits<Length>::max());

//     auto span = reservePages(pageCount, pageAlignment);
//     d_assert(isAligned(span, pageAlignment));

//     d_assert(contains(ptrFromOffset(span.offset)));
//   #ifndef NDEBUG
//     if (_mhIndex[span.offset].load().hasValue()) {
//       mesh::debug("----\n");
//       auto mh = reinterpret_cast<MiniHeap *>(miniheapForArenaOffset(span.offset));
//       mh->dumpDebug();
//     }
//   #endif

//     char *ptr = reinterpret_cast<char *>(ptrFromOffset(span.offset));

//     if (kAdviseDump) {
//       madvise(ptr, pageCount * kPageSize, MADV_DODUMP);
//     }

//     result = span;
//     return ptr;
//   }
