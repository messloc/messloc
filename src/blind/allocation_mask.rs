pub struct AllocationMask<const COUNT: usize> {
    mask: [u8; COUNT],
}

impl<const COUNT: usize> core::fmt::Debug for AllocationMask<COUNT> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for byte in &self.mask {
            write!(f, "{:08b}_", byte)?;
        }
        Ok(())
    }
}

impl<const COUNT: usize> AllocationMask<COUNT> {
    pub const fn new() -> Self {
        Self { mask: [0; COUNT] }
    }

    pub fn used(&mut self, offset: u8) {
        debug_assert!(offset < COUNT as u8 * 8);

        let bin = offset >> 3;
        let bin_offset = offset & 0b111;
        self.mask[bin as usize] |= 1 << bin_offset;
    }

    pub fn free(&mut self, offset: u8) {
        debug_assert!(offset < COUNT as u8 * 8);

        let bin = offset >> 3;
        let bin_offset = offset & 0b111;
        self.mask[bin as usize] &= !(1 << bin_offset);
    }

    pub fn is_free(&self, offset: u8) -> bool {
        debug_assert!(offset < COUNT as u8 * 8);

        let bin = offset >> 3;
        let bin_offset = offset & 0b111;
        (self.mask[bin as usize] & (1 << bin_offset)) == 0
    }

    pub fn free_iter(&self, count: u8) -> AllocationMaskFreeIter<'_, COUNT> {
        AllocationMaskFreeIter {
            mask: self,
            offset: 0,
            count,
        }
    }
}

pub struct AllocationMaskFreeIter<'a, const COUNT: usize> {
    mask: &'a AllocationMask<COUNT>,
    offset: u8,
    count: u8,
}

impl<'a, const COUNT: usize> Iterator for AllocationMaskFreeIter<'a, COUNT> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        debug_assert!(COUNT < u8::MAX as usize);

        loop {
            if self.offset == self.count {
                return None;
            }

            let offset = self.offset;
            self.offset += 1;

            if self.mask.is_free(offset) {
                return Some(self.offset);
            }
        }
    }
}

#[test]
fn allocation_mask() {
    let mut mask = AllocationMask::<5>::new();

    assert!(mask.is_free(0));
    assert!(mask.is_free(39));

    assert!(mask.is_free(23));
    mask.used(23);
    assert!(mask.is_free(22));
    assert!(!mask.is_free(23));
    assert!(mask.is_free(24));

    mask.free(23);
    assert!(mask.is_free(22));
    assert!(mask.is_free(23));
    assert!(mask.is_free(24));
}
