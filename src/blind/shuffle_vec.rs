#[derive(Debug)]
pub struct ShuffleVector<const COUNT: usize> {
    data: [u8; COUNT],
    offset: u8,
}

impl<const COUNT: usize> ShuffleVector<COUNT> {
    pub fn new<R: rand::Rng>(rng: &mut R) -> Self {
        // We need the last offset to be available for signalling an empty vector.
        assert!(COUNT < u8::MAX as usize);

        let mut count = 0;
        let mut data = [(); COUNT].map(|_| {
            let x = count;
            count += 1;
            x
        });

        for i in (1..COUNT).rev() {
            let j = rng.gen_range(0..=i);
            data.swap(i, j);
        }

        Self { data, offset: 0 }
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
    let mut shuffle = ShuffleVector::<5>::new(&mut rng);
    assert_eq!(shuffle.data, [3, 1, 4, 2, 0]);

    assert_eq!(shuffle.len(), 5);
    assert!(!shuffle.is_empty());

    assert_eq!(shuffle.pop(), Some(3));
    assert_eq!(shuffle.pop(), Some(1));
    assert_eq!(shuffle.pop(), Some(4));
    assert_eq!(shuffle.pop(), Some(2));

    assert_eq!(shuffle.len(), 1);
    assert!(!shuffle.is_empty());

    assert_eq!(shuffle.pop(), Some(0));

    assert_eq!(shuffle.len(), 0);
    assert!(shuffle.is_empty());

    assert_eq!(shuffle.pop(), None);

    assert_eq!(shuffle.len(), 0);
    assert!(shuffle.is_empty());
}

