use std::hash::{DefaultHasher, Hash, Hasher};
use std::marker::PhantomData;

use indexmap::IndexSet;

#[derive(Debug, Default, PartialEq)]
enum RingSetMode {
    #[default]
    Regular,
    HotToTop,
}

#[derive(Debug)]
struct RingSet<M> {
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

pub enum Message<M> {
    MemoryPage(M),
    RepairRequest(Vec<Bucket>),
}

pub struct MemoryLan<M> {
    cache: RingSet<M>,
    history: RingSet<Digest>,
}

impl<M> Default for MemoryLan<M>
where
    M: Clone + Eq + Hash,
{
    fn default() -> Self {
        Self::new(32, 64)
    }
}

impl<M> MemoryLan<M>
where
    M: Clone + Eq + Hash,
{
    pub fn new(cache_size: usize, history_size: usize) -> Self {
        Self {
            cache: RingSet::new(cache_size, RingSetMode::HotToTop),
            history: RingSet::new(history_size, RingSetMode::Regular),
        }
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.history.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn process(&mut self, message: Message<M>) {
        match message {
            Message::MemoryPage(memory_page) => self.incoming_memory_page(memory_page),
            Message::RepairRequest(filter) => {
                self.incoming_repair_request(filter);
            }
        }
    }

    pub fn request_slow_repair(&self) {
        todo!();
    }

    fn incoming_repair_request(&mut self, _filter: Vec<Bucket>) {
        todo!();
    }

    fn incoming_memory_page(&mut self, memory_page: M) {
        let hash = hash_digest(&memory_page);

        self.cache.push(memory_page);

        if self.history.push(hash).is_none() {
            return;
        }

        todo!();
    }
}

type Digest = u64;

fn hash_digest<T: Hash>(item: &T) -> Digest {
    let mut state = DefaultHasher::new(); // SipHasher 1-3
    item.hash(&mut state);
    state.finish()
}

type Fingerprint = u8;

type BucketIndex = usize;

const BUCKET_SIZE: usize = 4;
const MAX_EVICTIONS: usize = 32;

#[derive(Clone, Debug)]
pub struct Bucket(Vec<Fingerprint>);

impl Bucket {
    fn new() -> Self {
        Bucket(Vec::with_capacity(BUCKET_SIZE))
    }

    fn insert(&mut self, fingerprint: Fingerprint) -> bool {
        if self.0.len() < BUCKET_SIZE {
            self.0.push(fingerprint);
            true
        } else {
            false
        }
    }

    fn remove(&mut self, fingerprint: Fingerprint) -> bool {
        if let Some(index) = self.0.iter().position(|&fp| fp == fingerprint) {
            self.0.remove(index);
            true
        } else {
            false
        }
    }

    fn contains(&self, fingerprint: Fingerprint) -> bool {
        self.0.contains(&fingerprint)
    }

    #[allow(unused)]
    fn is_full(&self) -> bool {
        self.0.len() >= BUCKET_SIZE
    }

    fn clear(&mut self) {
        self.0.clear()
    }

    #[allow(unused)]
    fn len(&self) -> usize {
        self.0.len()
    }

    fn pop(&mut self) -> Option<Fingerprint> {
        if self.0.is_empty() {
            None
        } else {
            Some(self.0.remove(0))
        }
    }
}

fn compute_alt_index(hash: Digest, fingerprint: Fingerprint, num_buckets: usize) -> BucketIndex {
    let hash_2 = hash ^ hash_digest(&fingerprint);
    hash_2 as BucketIndex & (num_buckets - 1)
}

fn compute_args<T: ?Sized + Hash>(
    item: &T,
    num_buckets: usize,
) -> (Fingerprint, BucketIndex, BucketIndex) {
    let hash = hash_digest(&item);

    let fingerprint = hash as Fingerprint;
    let index = hash as BucketIndex & (num_buckets - 1);

    (
        fingerprint,
        index,
        compute_alt_index(hash, fingerprint, num_buckets),
    )
}

pub struct CuckooFilter<T>
where
    T: ?Sized + Hash,
{
    buckets: Vec<Bucket>,
    size: usize,
    _marker: PhantomData<T>,
}

impl<T> CuckooFilter<T>
where
    T: ?Sized + Hash,
{
    pub fn new(capacity: usize) -> Self {
        let num_buckets = std::cmp::max(1, capacity.next_power_of_two() / BUCKET_SIZE);
        let buckets = (0..num_buckets).map(|_| Bucket::new()).collect();

        Self {
            buckets,
            size: 0,
            _marker: PhantomData,
        }
    }

    pub fn from_buckets(buckets: Vec<Bucket>) -> Self {
        let size = {
            let mut result = 0;
            for bucket in &buckets {
                result += bucket.0.len();
            }
            result
        };

        Self {
            buckets,
            size,
            _marker: PhantomData,
        }
    }

    pub fn export(&self) -> Vec<Bucket> {
        self.buckets.clone()
    }

    pub fn insert(&mut self, item: &T) -> bool {
        let (fingerprint, index, alt_index) = compute_args(item, self.buckets.len());

        // Try to insert into first bucket.
        if self.buckets[index].insert(fingerprint) {
            self.size += 1;
            return true;
        }

        // Try to insert into second bucket.
        if self.buckets[alt_index].insert(fingerprint) {
            self.size += 1;
            return true;
        }

        // Both buckets are full, need to kick out.
        if self.insert_with_eviction(fingerprint, index, alt_index) {
            self.size += 1;
            return true;
        }

        false
    }

    fn insert_with_eviction(
        &mut self,
        mut fingerprint: Fingerprint,
        mut index: BucketIndex,
        mut alt_index: BucketIndex,
    ) -> bool {
        let num_buckets = self.buckets.len();

        for _ in 0..MAX_EVICTIONS {
            // Remove an item from a random bucket.
            let kick_bucket = if rand::random::<bool>() {
                index
            } else {
                alt_index
            };

            if let Some(old_fingerprint) = self.buckets[kick_bucket].pop() {
                self.buckets[kick_bucket].insert(fingerprint);

                // Find a new home for the removed fingerprint (cuckoo!).
                fingerprint = old_fingerprint;
                index = compute_alt_index(alt_index as Digest, fingerprint, num_buckets);
                alt_index = compute_alt_index(index as Digest, fingerprint, num_buckets);
            }
        }

        false // Failed after MAX_EVICTIONS attempts.
    }

    pub fn contains(&self, item: &T) -> bool {
        let (fingerprint, index, alt_index) = compute_args(item, self.buckets.len());
        self.buckets[index].contains(fingerprint) || self.buckets[alt_index].contains(fingerprint)
    }

    pub fn remove(&mut self, item: &T) -> bool {
        let (fingerprint, index, alt_index) = compute_args(item, self.buckets.len());

        if self.buckets[index].remove(fingerprint) {
            self.size -= 1;
            true
        } else if self.buckets[alt_index].remove(fingerprint) {
            self.size -= 1;
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        for bucket in self.buckets.iter_mut() {
            bucket.clear();
        }

        self.size = 0;
    }

    /// Returns the number of items in the filter.
    pub fn len(&self) -> usize {
        self.size
    }

    /// Checks if the filter is empty.
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
}

#[cfg(test)]
mod tests {
    use super::{Bucket, CuckooFilter, RingSet, RingSetMode};

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

    #[test]
    fn bucket_insert_and_len() {
        let mut bucket = Bucket::new();

        // Insert items and check length.
        bucket.insert(8);
        bucket.insert(9);
        assert_eq!(bucket.len(), 2);

        // Bucket size is limited to 4.
        bucket.insert(1);
        bucket.insert(2);
        assert!(!bucket.insert(3));
        assert!(!bucket.insert(4));
        assert_eq!(bucket.len(), 4);

        // Clear all items.
        bucket.clear();
        assert_eq!(bucket.len(), 0);
    }

    #[test]
    fn bucket_remove() {
        let mut bucket = Bucket::new();

        bucket.insert(1);
        assert!(bucket.contains(1));
        bucket.remove(1);
        assert!(!bucket.contains(1));
    }

    #[test]
    fn bucket_pop() {
        let mut bucket = Bucket::new();

        bucket.insert(2);
        bucket.insert(3);
        bucket.insert(4);
        bucket.insert(5);

        assert_eq!(bucket.pop(), Some(2));
        assert!(bucket.remove(3));
        assert!(!bucket.remove(3));
        assert_eq!(bucket.pop(), Some(4));
        assert_eq!(bucket.pop(), Some(5));
        assert_eq!(bucket.pop(), None);
    }

    #[test]
    fn cuckoo_insert_and_contains() {
        let mut filter = CuckooFilter::new(128);

        filter.insert(b"Pi");
        filter.insert(b"Pa");

        assert!(filter.contains(b"Pi"));
        assert!(filter.contains(b"Pa"));
        assert!(!filter.contains(b"Po"));
    }

    #[test]
    fn cuckoo_remove() {
        let mut filter = CuckooFilter::new(128);

        filter.insert(b"test");
        assert!(filter.contains(b"test"));

        filter.remove(b"test");
        assert!(!filter.contains(b"test"));
    }

    #[test]
    fn cuckoo_multiple_items() {
        let mut filter = CuckooFilter::new(128);
        let num = 64;

        for i in 0..num {
            let key = format!("key_{}", i);
            filter.insert(key.as_bytes());
        }

        assert_eq!(filter.len(), num);

        for i in 0..num {
            let key = format!("key_{}", i);
            assert!(filter.contains(key.as_bytes()));
        }

        // Matches number of fingerprints in filter.
        let mut fp_len = 0;
        for bucket in &filter.buckets {
            fp_len += bucket.len();
        }
        assert_eq!(filter.len(), fp_len);
    }

    #[test]
    fn cuckoo_len() {
        let mut filter = CuckooFilter::new(128);
        assert_eq!(filter.len(), 0);

        filter.insert(b"a");
        assert_eq!(filter.len(), 1);

        filter.insert(b"b");
        assert_eq!(filter.len(), 2);
    }

    #[test]
    fn cuckoo_import_export() {
        let mut filter = CuckooFilter::<&'static str>::new(128);
        filter.insert(&"bla");
        filter.insert(&"bli");
        filter.insert(&"blu");
        assert_eq!(filter.len(), 3);

        let export = filter.export();

        let filter_again = CuckooFilter::<String>::from_buckets(export);
        assert_eq!(filter_again.len(), 3);
        assert!(filter.contains(&"bla"));
        assert!(filter.contains(&"bli"));
        assert!(filter.contains(&"blu"));
    }
}
