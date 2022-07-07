//! This is a blind implementation taken directly from the paper describing the allocator
//! https://raw.githubusercontent.com/plasma-umass/Mesh/master/mesh-pldi19-powers.pdf
//!
//! The author's orginal code was not used as reference for this implementation.

pub mod allocation_mask;
pub mod mini_heap;
pub mod shuffle_vec;
pub mod span;
pub mod linked_heap;
use std::{alloc::Layout, ptr::NonNull, mem::MaybeUninit};

use allocation_mask::*;
use linked_heap::*;
use mini_heap::*;
use shuffle_vec::*;
use span::*;

const SIZE_CLASS_COUNT: usize = 25;

static SIZE_CLASSES: [usize; SIZE_CLASS_COUNT] = [
    8, 16, 32, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896,
    1024, 2048, 4096, 8192, 16384,
];
