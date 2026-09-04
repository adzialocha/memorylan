use std::hash::Hash;

use indexmap::IndexSet;

#[derive(Debug, Default, PartialEq)]
pub enum RingSetMode {
    #[default]
    Regular,
    HotToTop,
}

#[derive(Debug)]
pub struct RingSet<M> {
    capacity: usize,
    mode: RingSetMode,
    set: IndexSet<M>,
}

impl<M> RingSet<M>
where
    M: Clone + Eq + Hash,
{
    pub fn new(capacity: usize, mode: RingSetMode) -> Self {
        Self {
            capacity,
            mode,
            set: IndexSet::with_capacity_and_hasher(capacity, <_>::default()), // SipHasher 1-3
        }
    }

    pub fn contains(&self, item: &M) -> bool {
        self.set.contains(item)
    }

    pub fn push(&mut self, item: M) -> Option<M> {
        match self.mode {
            RingSetMode::Regular => {
                if self.contains(&item) {
                    return None;
                }
            }
            RingSetMode::HotToTop => {
                if self.set.shift_remove(&item) {
                    self.set.insert(item);
                    return None;
                }
            }
        }

        if self.set.len() == self.capacity {
            if let Some(oldest) = self.set.first().cloned() {
                self.set.shift_remove(&oldest);
                self.set.insert(item);
                return Some(oldest);
            }
        } else {
            self.set.insert(item);
        }

        None
    }

    pub fn clear(&mut self) {
        self.set.clear();
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{RingSet, RingSetMode};

    #[test]
    fn ring_set_regular() {
        let mut ring = RingSet::new(3, RingSetMode::default());

        assert_eq!(ring.push(1), None); // [1]
        assert_eq!(ring.push(2), None); // [1, 2]
        assert_eq!(ring.push(1), None); // [1, 2]
        assert!(ring.contains(&1));
        assert_eq!(ring.len(), 2);
    }

    #[test]
    fn ring_set_hot_mode() {
        let mut ring = RingSet::new(3, RingSetMode::HotToTop);

        assert_eq!(ring.push(1), None); // [1]
        assert_eq!(ring.push(2), None); // [1, 2]
        assert!(ring.contains(&1));

        // Re-inserting 1 moves it to the back.
        assert_eq!(ring.push(1), None); // [2, 1]

        // Reaching capacity will dequeue oldest item.
        assert_eq!(ring.push(3), None); // [2, 1, 3]
        assert_eq!(ring.push(4), Some(2)); // [1, 3, 4]
    }
}
