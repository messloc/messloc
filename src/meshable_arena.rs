use libc::c_void;

use crate::{cheap_heap::CheapHeap, ARENA_SIZE, MIN_ARENA_EXPANSION, PAGE_SIZE, SPAN_CLASS_COUNT, mini_heap::{AtomicMiniHeapId}};
use std::{
    pointer::offset_from,
    process::id,
    ptr::null_mut,
    sync::atomic::Ordering, path::PathBuf, fs::create_dir_all
};

use crate::utils::*;
const TMP_DIR: &str = "/tmp";


#[derive(Clone, Debug, Default)]
pub struct Span {
    pub offset: Offset,
    pub length: Length,
}
pub type Offset = u32;
pub type Length = u32;

impl Span {
    fn new(offset: u32, length: u32) -> Span {
        Span { offset, length }
    }

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

//FIXME: Move

pub struct MeshableArena {
    pub(crate) arena_begin: *mut Page,
    fd: i32,
    /// offset in pages
    end: Offset,
    dirty: [arrayvec::ArrayVec<Span, 1024>; SPAN_CLASS_COUNT as usize],
    clean: [arrayvec::ArrayVec<Span, 1024>; SPAN_CLASS_COUNT as usize],
    mh_index: AtomicMiniHeapId,
    pub(crate) mh_allocator: CheapHeap<64, { ARENA_SIZE / PAGE_SIZE }>,
    meshed_bitmap: MeshedBitmap,
    freed_spans: [arrayvec::ArrayVec<Span, 1024>; SPAN_CLASS_COUNT as usize],
    span_dir: PathBuf,
    dirty_page_threshold: usize,
    meshed_page_count_hwm: usize,
}

impl MeshableArena {
    pub fn new() -> MeshableArena {
        //TODO: initialise stuff from the constructor
        todo!()
    }

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

        debug_assert!(!result.is_empty());
        debug_assert_ne!(flags, PageType::Unknown);

        let ptr = unsafe { self.arena_begin.add(result.offset as usize) };
        if page_align > 1 && ptr.align_offset(page_align) != 0 {
            todo!("page wasn't aligned")

            // freeSpan(result, flags);
            // // recurse once, asking for enough extra space that we are sure to
            // // be able to find an aligned offset of pageCount pages within.
            // result = reservePages(pageCount + 2 * pageAlignment, 1);

            // const size_t alignment = pageAlignment * kPageSize;
            // const uintptr_t alignedPtr = (ptrvalFromOffset(result.offset) + alignment - 1) & ~(alignment - 1);
            // const auto alignedOff = offsetFor(reinterpret_cast<void *>(alignedPtr));
            // d_assert(alignedOff >= result.offset);
            // d_assert(alignedOff < result.offset + result.length);
            // const auto unwantedPageCount = alignedOff - result.offset;
            // auto alignedResult = result.splitAfter(unwantedPageCount);
            // d_assert(alignedResult.offset == alignedOff);
            // freeSpan(result, flags);
            // const auto excess = alignedResult.splitAfter(pageCount);
            // freeSpan(excess, flags);
            // result = alignedResult;
        }

        result
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

        // invariant should be maintained
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

    pub unsafe fn track_miniheap(&mut self, span: Span, id: u32) {
        for i in 0..span.length {
            self.set_index((span.offset + i) as usize, id)
        }
    }

    pub unsafe fn set_index(&mut self, offset: usize, id: u32) {
        (&*self.mh_index.add(offset)).store(id, Release);
    }

    fn scavenge(&self, force: bool) {
        if force && self.dirty.len() < self.dirty_page_threshold {
            let mut bitmap = Bitmap::new();
            bitmap.invert();

            self.freed_spans.iter().for_each(|span_list| {
                span_list.iter().enumerate().for_each(|(key, span)| {
                   self.meshed_bitmap.unset(span.offset as usize + key);
                   (0..=span.length).for_each(|k| bitmap.try_set(span.offset + k)); 
                    self.meshed_bitmap.reset_span_mapping(span);
                })
            });

            self.freed_spans.clear();

            let page_count = self.mashed_bitmap.in_use_count();
            if page_count > self.meshed_page_count_hwm {
                self.meshed_page_count_hwm = page_count;
            }

            for_each_free(self.dirty, |span| {
                let ptr = unsafe {self.arena_begin.add(span.offset)};
                let size = span.byte_length();
                self.free_physical(ptr, span.offset as usize, size);
                (0..=span.length).for_each(|k| bitmap.try_set(span.offset + k)); 
            });

            self.dirty.clear();
            self.clean.clear();

            self.coalesce(bitmap);
        }
    }

    fn coalesce(&self, bitmap: Bitmap) {
        let current = Span::default();
        for i in bitmap.iter() {
            if i == current.offset + current.length {
                current.length += 1;
                continue;
            }

            if !current.is_empty() {
                self.clean
                    .get_mut(current.class() as usize)
                    .unwrap()
                    .push(current);
            }

            current = Span::new(i, 1);
        }
    }

    fn free_physical(&self, ptr: *mut [u8] , offset: usize, size: usize) {
        let ptr = unsafe { self.arena_begin.add(offset) };

        assert!(size / crate::PAGE_SIZE > 0);
        assert!(size % crate::PAGE_SIZE > 0);

        //TODO:: add check for if meshing is enabled or not
        let _ = unsafe { fallocate(self.fd,offset, size) };
        
    }

    fn partial_scavenge(&self) {
        for_each_free(self.dirty, |span| {
            let ptr = unsafe { self.arena_begin.add(span.offset) };
            let size = span.byte_length();
            unsafe { madvise(ptr as *mut c_void, size) };
            self.free_physical(ptr, span.offset, size);
            self.clean.get_mut(span.span_class).unwrap().push(span);
        }); 

        self.dirty.clear();
    }

    fn begin_mesh(&self, remove: *mut [u8], size: usize) {
        let _ = unsafe { mprotect(remove as *mut c_void, size).unwrap() };
    }

    fn finalise_mesh(&self, keep: *mut (), remove: *mut (), size: usize) {
        let keep_offset = unsafe { keep.offset_from(self.arena_begin as *mut ()) };
        let remove_offset = unsafe { remove.offset_from(self.arena_begin as *mut ())};
        let page_count = size / PAGE_SIZE;
        let keep_id = self.mh_index.get(usize::try_from(keep_offset).unwrap()).unwrap().load(Ordering::Acquire);
        self.store_indices(keep_id, remove_offset, page_count);
        let removed_span = Span::new(u32::try_from(remove_offset).unwrap(), u32::try_from(page_count).unwrap());
        self.track_meshed(removed_span);
        let _ = unsafe { mmap(remove as *mut c_void, self.fd, size, usize::try_from(keep_offset).unwrap() * PAGE_SIZE)};

    }

    fn store_indices(&self, keep_id: u32, offset: isize, page_count: usize) {
        (0..page_count).for_each(|index| {
            self.mh_index.get(index).unwrap().store(keep_id, Ordering::Release);
    });
    }

    fn open_shm_span_file(&mut self, size: usize) -> i32{
        self.span_dir = self.open_span_dir().unwrap();
        // this is required for mkstemp
        self.span_dir.push("XXXXXX");
        unsafe {
        let fd = mkstemp(&self.span_dir).unwrap();
        let _ = unlink(&self.span_dir).unwrap();
        let _ = ftruncate(fd, size).unwrap();
        let _ = fcntl(fd);
        fd
        }
    }
 
    fn open_span_dir(&self) -> Option<PathBuf> {
        let pid = id();
        let mut i = 1;
        loop {
            let mut path = PathBuf::from(TMP_DIR);
            path.join(format!("alloc-mesh-{pid}.{i}"));
            if create_dir_all(path).is_ok() {
               return Some(path); 
            } else if i >= 1024 {
                break;
            } else {
                i += 1;
            }
    };
        None
    }
}

#[derive(PartialEq, Debug, Clone, Copy)]
enum PageType {
    Clean = 0,
    Dirty = 1,
    Meshed = 2,
    Unknown = 3,
}
