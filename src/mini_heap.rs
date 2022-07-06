use std::ptr::{addr_of, addr_of_mut};

use crate::meshable_arena::{Page, Span};

pub struct MiniHeap {
    bitmap: (),
    span: Span,
    //   internal::Bitmap _bitmap;           // 32 bytes 32
    //   const Span _span;                   // 8        40
    //   MiniHeapListEntry _freelist{};      // 8        48
    //   atomic<pid_t> _current{0};          // 4        52
    //   Flags _flags;                       // 4        56
    //   const float _objectSizeReciprocal;  // 4        60
    //   MiniHeapID _nextMeshed{};           // 4        64
}

impl MiniHeap {
    // creates the MiniHeap at the location of the pointer
    pub unsafe fn new_inplace(
        this: *mut Self,
        arena: *mut Page,
        span: Span,
        object_count: usize,
        object_size: usize,
    ) {
        addr_of_mut!((*this).span).write(span);
    }
}
// //   MiniHeap(void *arenaBegin, Span span, size_t objectCount, size_t objectSize)
//   : _bitmap(objectCount),
//     _span(span),
//     _flags(objectCount, objectCount > 1 ? SizeMap::SizeClass(objectSize) : 1, 0, list::Attached),
//     _objectSizeReciprocal(1.0 / (float)objectSize) {
// // debug("sizeof(MiniHeap): %zu", sizeof(MiniHeap));

// d_assert(_bitmap.inUseCount() == 0);

// const auto expectedSpanSize = _span.byteLength();
// d_assert_msg(expectedSpanSize == spanSize(), "span size %zu == %zu (%u, %u)", expectedSpanSize, spanSize(),
//              maxCount(), this->objectSize());

// // d_assert_msg(spanSize == static_cast<size_t>(_spanSize), "%zu != %hu", spanSize, _spanSize);
// // d_assert_msg(objectSize == static_cast<size_t>(objectSize()), "%zu != %hu", objectSize, _objectSize);

// d_assert(!_nextMeshed.hasValue());

// // debug("new:\n");
// // dumpDebug();
// }
