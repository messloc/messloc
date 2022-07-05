use std::ptr::null_mut;

use crate::{MIN_ARENA_EXPANSION, PAGE_SIZE, SPAN_CLASS_COUNT};

#[derive(Default, Clone, Copy)]
pub struct Span {
    offset: Offset,
    length: Length,
}
pub type Offset = u32;
pub type Length = u32;

impl Span {
    fn class(self) -> u32 {
        Length::min(self.length, SPAN_CLASS_COUNT) - 1
    }

    fn split_after(&mut self, page_count: Length) -> Self {
        debug_assert!(page_count <= self.length);
        let rest_page_count = self.length - page_count;
        self.length = page_count;
        Span {
            offset: self.offset + page_count,
            length: rest_page_count,
        }
    }

    fn is_empty(self) -> bool {
        self.length == 0
    }
}

pub type Page = [u8; PAGE_SIZE];

pub struct MeshableArena {
    arena_begin: *mut Page,
    /// offset in pages
    end: Offset,
    dirty: [arrayvec::ArrayVec<Span, 1024>; SPAN_CLASS_COUNT as usize],
    clean: [arrayvec::ArrayVec<Span, 1024>; SPAN_CLASS_COUNT as usize],
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
        debug_assert!(page_count > 0);

        let (result, flags) = match self.find_pages(page_count as u32) {
            Some((span, flags)) => (span, flags),
            None => {
                self.expand_arena(page_count);
                self.find_pages(page_count as u32).unwrap() // unchecked?
            }
        };

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

    pub fn expand_arena(&mut self, min_pages_added: usize) {
        let page_count = usize::max(min_pages_added, MIN_ARENA_EXPANSION);

        let expansion = Span {
            offset: self.end,
            length: page_count as u32,
        };
        self.end += page_count as u32;

        //   if (unlikely(_end >= kArenaSize / kPageSize)) {
        //     debug("Mesh: arena exhausted: current arena size is %.1f GB; recompile with larger arena size.",
        //           kArenaSize / 1024.0 / 1024.0 / 1024.0);
        //     abort();
        //   }

        self.clean[expansion.class() as usize].push(expansion)

        //   _clean[expansion.spanClass()].push_back(expansion);
    }

    fn find_pages(&mut self, page_count: u32) -> Option<(Span, PageType)> {
        // Search through all dirty spans first.  We don't worry about
        // fragmenting dirty pages, as being able to reuse dirty pages means
        // we don't increase RSS.
        let span = Span {
            offset: 0,
            length: page_count,
        };
        for span_class in span.class()..SPAN_CLASS_COUNT {
            if let Some(span) = Self::find_pages_inner(&mut self.dirty, span_class, page_count) {
                return Some((span, PageType::Dirty));
            }
        }

        // if no dirty pages are available, search clean pages.  An allocated
        // clean page (once it is written to) means an increased RSS.
        for span_class in span.class()..SPAN_CLASS_COUNT {
            if let Some(span) = Self::find_pages_inner(&mut self.clean, span_class, page_count) {
                return Some((span, PageType::Clean));
            }
        }

        None
    }

    fn find_pages_inner(
        free_spans: &mut [arrayvec::ArrayVec<Span, 1024>; SPAN_CLASS_COUNT as usize],
        span_class: u32,
        page_count: u32,
    ) -> Option<Span> {
        let spans = &mut free_spans[span_class as usize];
        if spans.is_empty() {
            return None;
        }

        // let old_len = spans.len();
        let end = spans.len() - 1;

        if span_class == SPAN_CLASS_COUNT - 1 && spans[end].length < page_count {
            // the final span class contains (and is the only class to
            // contain) variable-size spans, so we need to make sure we
            // search through all candidates in this case.
            for j in 0..end {
                if spans[j].length >= page_count {
                    spans.swap(j, end);
                    break;
                }
            }

            // check that we found something in the above loop. this would be
            // our last loop iteration anyway
            if spans[end].length < page_count {
                return None;
            }
        }

        let mut span = spans.pop().unwrap();

        // #ifndef NDEBUG
        // d_assert_msg(oldLen == spanList.size() + 1, "pageCount:%zu,%zu -- %zu/%zu", pageCount, i, oldLen, spanList.size());
        // for (size_t j = 0; j < spanList.size(); j++) {
        // d_assert(spanList[j] != span);
        // }
        // #endif

        // this invariant should be maintained
        debug_assert!(span.length > span_class);
        debug_assert!(span.length >= page_count);

        // put the part we don't need back in the reuse pile
        let rest = span.split_after(page_count);
        if !rest.is_empty() {
            free_spans[rest.class() as usize].push(rest);
        }
        debug_assert_eq!(span.length, page_count);

        Some(span)
    }
}

enum PageType {
    Clean = 0,
    Dirty = 1,
    Meshed = 2,
    Unknown = 3,
}
