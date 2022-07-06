
pub const PAGE_SIZE: usize = 4096;
pub const DATA_LEN: usize = 128;
pub const ARENA_SIZE: usize = PAGE_SIZE * 2;
pub const SPAN_CLASS_COUNT: u32 = 256;
pub const MIN_ARENA_EXPANSION: usize = 4096;  // 16 MB in pages

pub const CACHELINE_SIZE: usize = 64;


// ensures we amortize the cost of going to the global heap enough
pub const MAX_MINIHEAPS_PER_SHUFFLE_VECTOR: usize = 24; 

// shuffle vector features
pub const MAX_SHUFFLE_VECTOR_LENGTH: usize = 256; // sizeof(u8) << 8


pub const ENABLE_SHUFFLE_ON_FREE: bool = true;