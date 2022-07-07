use core::sync::atomic::Ordering;

pub struct AllocationMask<const COUNT: usize> {
    mask: [core::sync::atomic::AtomicU8; COUNT],
}

impl<const COUNT: usize> core::fmt::Debug for AllocationMask<COUNT> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for byte in &self.mask {
            write!(f, "{:08b}_", byte.load(Ordering::Relaxed))?;
        }
        Ok(())
    }
}

impl<const COUNT: usize> AllocationMask<COUNT> {
    pub fn new() -> Self {
        Self {
            mask: [(); COUNT].map(|_| core::sync::atomic::AtomicU8::new(0)),
        }
    }

    pub fn set(&self, offset: u8) {
        let bin = offset >> 3;
        let bin_offset = offset & 0b111;
        self.mask[bin as usize].fetch_or(1 << bin_offset, Ordering::Relaxed);
    }
}
