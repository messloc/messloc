use std::ptr::null_mut;

use crate::PAGE_SIZE;

#[derive(Default)]
pub struct Span {
    offset: Offset,
    length: Length,
}
pub type Offset = u32;
pub type Length = u32;

pub type Page = [u8; PAGE_SIZE];

pub struct MeshableArena {
    arena_begin: *mut Page,
}

impl MeshableArena {
    pub fn page_alloc(&mut self, page_count: usize, page_align: usize) -> (Span, *mut Page) {
        if page_count == 0 {
            return (Span::default(), null_mut());
        }

        debug_assert_ne!(
            self.arena_begin,
            null_mut(),
            "meshable arena must be initialised"
        );

        debug_assert!(page_count > 0);
        debug_assert!(page_count < Length::MAX as usize);

        let span = self.reserve_pages(page_count, page_align);

        //     d_assert(isAligned(span, pageAlignment));
        //     d_assert(contains(ptrFromOffset(span.offset)));
        //   #ifndef NDEBUG
        //     if (_mhIndex[span.offset].load().hasValue()) {
        //       mesh::debug("----\n");
        //       auto mh = reinterpret_cast<MiniHeap *>(miniheapForArenaOffset(span.offset));
        //       mh->dumpDebug();
        //     }
        //   #endif

        // Safety: the span offset resides within our total arena size
        let ptr = unsafe { self.arena_begin.add(span.offset as usize) };

        //     if (kAdviseDump) {
        //       madvise(ptr, pageCount * kPageSize, MADV_DODUMP);
        //     }

        (span, ptr)
    }

    fn reserve_pages(&mut self, page_count: usize, page_align: usize) -> Span {
        // d_assert(pageCount >= 1);

        // internal::PageType flags(internal::PageType::Unknown);
        // Span result(0, 0);
        // auto ok = findPages(pageCount, result, flags);
        // if (!ok) {
        //   expandArena(pageCount);
        //   ok = findPages(pageCount, result, flags);
        //   hard_assert(ok);
        // }

        // d_assert(!result.empty());
        // d_assert(flags != internal::PageType::Unknown);

        // if (unlikely(pageAlignment > 1 && ((ptrvalFromOffset(result.offset) / kPageSize) % pageAlignment != 0))) {
        //   freeSpan(result, flags);
        //   // recurse once, asking for enough extra space that we are sure to
        //   // be able to find an aligned offset of pageCount pages within.
        //   result = reservePages(pageCount + 2 * pageAlignment, 1);

        //   const size_t alignment = pageAlignment * kPageSize;
        //   const uintptr_t alignedPtr = (ptrvalFromOffset(result.offset) + alignment - 1) & ~(alignment - 1);
        //   const auto alignedOff = offsetFor(reinterpret_cast<void *>(alignedPtr));
        //   d_assert(alignedOff >= result.offset);
        //   d_assert(alignedOff < result.offset + result.length);
        //   const auto unwantedPageCount = alignedOff - result.offset;
        //   auto alignedResult = result.splitAfter(unwantedPageCount);
        //   d_assert(alignedResult.offset == alignedOff);
        //   freeSpan(result, flags);
        //   const auto excess = alignedResult.splitAfter(pageCount);
        //   freeSpan(excess, flags);
        //   result = alignedResult;
        // }

        // return result;
        Span::default()
    }

    //   inline void *ptrFromOffset(size_t off) const {
    //     return reinterpret_cast<void *>(ptrvalFromOffset(off));
    //   }
}
