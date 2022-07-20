use super::allocation_mask::{AllocationMask, AllocationMaskFreeIter};

pub struct ShuffleVector<const COUNT: usize> {
    data: [u8; COUNT],
    offset: u8,
}

impl<const COUNT: usize> core::fmt::Debug for ShuffleVector<COUNT> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(&self.data[0..self.offset as usize])
            .finish()
    }
}

impl<const COUNT: usize> ShuffleVector<COUNT> {
    pub fn new() -> Self {
        // We need the last offset to be available for signalling an empty vector.
        assert!(COUNT < u8::MAX as usize);

        Self {
            data: [0; COUNT],
            offset: 0,
        }
    }

    pub fn pop(&mut self) -> Option<u8> {
        if self.is_empty() {
            return None;
        }

        self.offset -= 1;

        Some(self.data[self.offset as usize])
    }

    pub fn push<R: rand::Rng>(&mut self, rng: &mut R, item: u8) -> Option<u8> {
        if self.offset == COUNT as u8 {
            // vec is full
            return Some(item);
        }

        self.data[self.offset as usize] = item;

        if self.offset != 0 {
            let i = self.offset as usize;
            let j = rng.gen_range(0..self.offset);
            self.data.swap(i, j as usize);
        }

        self.offset += 1;

        None
    }

    pub fn fill<R: rand::Rng, const MASK_SIZE: usize>(
        &mut self,
        rng: &mut R,
        mut allocation_mask_iter: AllocationMaskFreeIter<'_, MASK_SIZE>,
    ) {
        debug_assert!(COUNT <= MASK_SIZE * 8);
        debug_assert!(COUNT < u8::MAX as usize);

        let mut count: u8 = 0;
        for (i, offset) in allocation_mask_iter.take(COUNT).enumerate() {
            count += 1;
            self.data[i] = offset;
        }

        // https://en.wikipedia.org/wiki/Fisher%E2%80%93Yates_shuffle
        for i in (0..count).rev() {
            let j = rng.gen_range(0..=i);
            self.data.swap(i as usize, j as usize);
        }

        self.offset = count;
    }

    pub fn len(&self) -> usize {
        self.offset as usize
    }

    pub fn is_empty(&self) -> bool {
        self.offset == 0
    }
}

#[test]
fn test_shuffle_vec() {
    let mut rng = rand::rngs::mock::StepRng::new(109283091824091824, 10293801982340293745);

    let mut shuffle = ShuffleVector::<5>::new();

    let mask = AllocationMask::<1>::new();
    shuffle.fill(&mut rng, mask.free_iter(5));

    assert_eq!(shuffle.data, [2, 1, 3, 5, 4]);

    // starts with 5 elements
    assert_eq!(shuffle.len(), 5);
    assert!(!shuffle.is_empty());

    // can't push a 6th element
    assert_eq!(shuffle.push(&mut rng, 6), Some(6));

    // pop 2 then push 1 then pop 4
    assert_eq!(shuffle.pop(), Some(4));
    assert_eq!(shuffle.pop(), Some(5));
    assert_eq!(shuffle.push(&mut rng, 5), None);
    assert_eq!(shuffle.pop(), Some(1));
    assert_eq!(shuffle.pop(), Some(3));
    assert_eq!(shuffle.pop(), Some(5));

    // we have one item left
    assert_eq!(shuffle.len(), 1);
    assert!(!shuffle.is_empty());

    // pop the last item
    assert_eq!(shuffle.pop(), Some(2));

    // no items left
    assert_eq!(shuffle.len(), 0);
    assert!(shuffle.is_empty());

    // can't remove any more items
    assert_eq!(shuffle.pop(), None);

    // vec remains empty
    assert_eq!(shuffle.len(), 0);
    assert!(shuffle.is_empty());

    // pushing into the vec after being empty adds it to the end
    assert_eq!(shuffle.push(&mut rng, 3), None);
    assert_eq!(shuffle.data, [3, 5, 3, 1, 4]);
}
