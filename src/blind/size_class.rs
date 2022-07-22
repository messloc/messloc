/// Number of class sizes.
pub const CLASS_SIZE_COUNT: usize = 24;

/// These are the size classes used by jemalloc (http://jemalloc.net/jemalloc.3.html)
/// up to 1024 and excluding the 8 byte bin. The 8 byte bin was removed to improve the performance
/// of `find_size_class_index`, and because a 8 byte or less allocation should be rare.
/// After 1024, the bin sizes go up by powers of 2 until 2^14.
pub const CLASS_SIZES: [u16; CLASS_SIZE_COUNT] = [
    16, 32, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896, 1024,
    2048, 4096, 8192, 16384,
];

/// Find the index into the `CLASS_SIZES` array for the given size.
///
/// The implementation of this function uses two lookup tables.
/// This has been benchmarked to be faster than using `.position()` or `if` based binary search.
/// The benchmark also showed this method to match a hand made lookup table.
pub fn find_size_class_index(size: usize) -> Option<usize> {
    // Correction for less than or equal binning.
    // Otherwise, 32 would be in 48.
    let size_minus_one = size.saturating_sub(1);

    // Perform manual log2 of `size_minus_one` with a 128 offset.
    // This finds the the 0..=1024 small bins.
    match size_minus_one >> 7 {
        0 => Some(size_minus_one >> 4),
        1 => Some((size_minus_one >> 5) + 4),
        2 | 3 => Some((size_minus_one >> 6) + 8),
        4 | 5 | 6 | 7 => Some((size_minus_one >> 7) + 12),

        // Perform manual log2 of `size_minus_one` with a 2048 offset.
        // This finds the the 1025..=16384 medium bins.
        x => match x >> 4 {
            0 => Some(20),
            1 => Some(21),
            2 | 3 => Some(22),
            4 | 5 | 6 | 7 => Some(23),
            _ => None,
        },
    }
    // class_array.get(class_index(size)).copied()
}

#[test]
fn test_find_size_class_index() {
    // Test all sizes 0..=16384.
    for size in 0..=2_usize.pow(14) {
        let truth = CLASS_SIZES
            .iter()
            .position(|&class_size| size <= class_size as usize)
            .expect(&format!("{} to fit in {:?}", size, CLASS_SIZES));

        let result = find_size_class_index(size);

        assert_eq!(
            result,
            Some(truth),
            "Given {}, size class {} was returned",
            size,
            CLASS_SIZES[result.expect(&format!("Size {} was not in the size classes", size))]
        );
    }

    // Test 16385.
    assert_eq!(
        find_size_class_index(2_usize.pow(14) + 1),
        None,
        "Expected 16K+ to not be in the size classes"
    );
}
