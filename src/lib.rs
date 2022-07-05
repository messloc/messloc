mod global_heap;
mod cheap_heap;
mod meshable_arena;

const PAGE_SIZE: usize = 4096;
const DATA_LEN: usize = 128;
const ARENA_SIZE: usize = PAGE_SIZE * 2;

const SPAN_CLASS_COUNT: u32 = 256;
const MIN_ARENA_EXPANSION: usize = 4096;  // 16 MB in pages

pub struct MiniHeap; // stub
