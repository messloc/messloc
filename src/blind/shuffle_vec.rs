pub struct ShuffleVector<const COUNT: usize> {
    data: [u8; COUNT],
    offset: u8,
}

impl<const COUNT: usize> core::fmt::Debug for ShuffleVector<COUNT> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(&self.data[self.offset as usize..])
            .finish()
    }
}

impl<const COUNT: usize> ShuffleVector<COUNT> {
    pub fn new<R: rand::Rng>(rng: &mut R, length: u8) -> Self {
        // We need the last offset to be available for signalling an empty vector.
        assert!((COUNT - 1) < u8::MAX as usize);

        let mut count = COUNT as u8 - 1;
        let mut data = [(); COUNT].map(|_| {
            let x = count;
            if count > 0 {
                count -= 1;
            }
            x
        });

        let offset = COUNT - length as usize;

        // https://en.wikipedia.org/wiki/Fisher%E2%80%93Yates_shuffle
        for i in (offset..COUNT).rev() {
            let j = rng.gen_range(offset..=i);
            data.swap(i, j);
        }

        Self {
            data,
            offset: offset as u8,
        }
    }

    pub fn pop(&mut self) -> Option<u8> {
        if self.is_empty() {
            return None;
        }

        let item = self.data[self.offset as usize];

        self.offset += 1;
        if self.offset as usize >= COUNT {
            // There are no items left
            self.offset = u8::MAX;
        }

        Some(item)
    }

    pub fn push<R: rand::Rng>(&mut self, rng: &mut R, item: u8) -> Option<u8> {
        if self.offset == 0 {
            // vec is full
            return Some(item);
        }

        if self.is_empty() {
            self.offset = (COUNT - 1) as u8;
        } else {
            self.offset -= 1;
        }

        self.data[self.offset as usize] = item;

        let i = self.offset as usize;
        let j = rng.gen_range(i..COUNT);
        self.data.swap(i, j);

        None
    }

    pub fn len(&self) -> usize {
        if self.is_empty() {
            0
        } else {
            COUNT - self.offset as usize
        }
    }

    pub fn is_empty(&self) -> bool {
        self.offset == u8::MAX
    }
}

#[test]
fn test_shuffle_vec() {
    let mut rng = rand::rngs::mock::StepRng::new(109283091824091824, 10293801982340293745);
    // let mut rng = rand::thread_rng();
    let mut shuffle = ShuffleVector::<7>::new(&mut rng, 3);
    assert_eq!(shuffle.data, [6, 5, 4, 3, 0, 1, 2]);

    let mut shuffle = ShuffleVector::<5>::new(&mut rng, 5);
    assert_eq!(shuffle.data, [2, 4, 3, 0, 1]);

    // starts with 5 elements
    assert_eq!(shuffle.len(), 5);
    assert!(!shuffle.is_empty());

    // can't push a 6th element
    assert_eq!(shuffle.push(&mut rng, 6), Some(6));

    // pop 2 then push 1 then pop 4
    assert_eq!(shuffle.pop(), Some(2));
    assert_eq!(shuffle.pop(), Some(4));
    assert_eq!(shuffle.push(&mut rng, 4), None);
    assert_eq!(shuffle.pop(), Some(3));
    assert_eq!(shuffle.pop(), Some(4));
    assert_eq!(shuffle.pop(), Some(0));

    // we have one item left
    assert_eq!(shuffle.len(), 1);
    assert!(!shuffle.is_empty());

    // pop the last item
    assert_eq!(shuffle.pop(), Some(1));

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
    assert_eq!(shuffle.data, [2, 3, 4, 0, 3]);
}
