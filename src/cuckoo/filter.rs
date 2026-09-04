use std::hash::Hash;
use std::marker::PhantomData;

use crate::cuckoo::utils::{alt_index, fingerprint_index};
use crate::cuckoo::{Bucket, BucketIndex, DEFAULT_BUCKET_SIZE, DEFAULT_MAX_EVICTIONS, Fingerprint};

pub struct CuckooFilter<T>
where
    T: ?Sized + Hash,
{
    buckets: Vec<Bucket>,
    size: usize,
    max_evictions: usize,
    bucket_size: usize,
    _marker: PhantomData<T>,
}

impl<T> CuckooFilter<T>
where
    T: ?Sized + Hash,
{
    pub fn new(capacity: usize) -> Self {
        let bucket_size = DEFAULT_BUCKET_SIZE;
        let max_evictions = DEFAULT_MAX_EVICTIONS;

        let num_buckets = std::cmp::max(1, capacity.next_power_of_two() / bucket_size);
        let buckets = (0..num_buckets).map(|_| Bucket::new(bucket_size)).collect();

        Self {
            buckets,
            size: 0,
            max_evictions,
            bucket_size,
            _marker: PhantomData,
        }
    }

    pub fn insert(&mut self, item: &T) -> bool {
        let num_buckets = self.buckets.len();

        let (fp, i1) = fingerprint_index(item, num_buckets);
        if self.buckets[i1].insert(fp) {
            self.size += 1;
            return true;
        }

        let i2 = alt_index(fp, i1, num_buckets);
        if self.buckets[i2].insert(fp) {
            self.size += 1;
            return true;
        }

        // Remove an item from a random bucket.
        let evicted = if rand::random::<bool>() { i1 } else { i2 };
        if self.insert_with_eviction(fp, evicted) {
            self.size += 1;
            return true;
        }

        false
    }

    fn insert_with_eviction(&mut self, mut fp: Fingerprint, mut i: BucketIndex) -> bool {
        let num_buckets = self.buckets.len();

        for _ in 0..self.max_evictions {
            if let Some(evicted_fp) = self.buckets[i].force_insert(fp) {
                // Find a new home for the removed fingerprint (cuckoo!).
                fp = evicted_fp;
                i = alt_index(fp, i, num_buckets);
            } else {
                return true;
            }
        }

        false // Failed after too many insertion attempts.
    }

    pub fn contains(&self, item: &T) -> bool {
        let num_buckets = self.buckets.len();

        let (fp, i1) = fingerprint_index(item, num_buckets);
        if self.buckets[i1].contains(fp) {
            return true;
        }

        let i2 = alt_index(fp, i1, num_buckets);
        self.buckets[i2].contains(fp)
    }

    pub fn remove(&mut self, item: &T) -> bool {
        let num_buckets = self.buckets.len();

        let (fp, i1) = fingerprint_index(item, num_buckets);
        if self.buckets[i1].remove(fp) {
            self.size -= 1;
            return true;
        }

        let i2 = alt_index(fp, i1, num_buckets);
        if self.buckets[i2].remove(fp) {
            self.size -= 1;
            return true;
        }

        false
    }

    pub fn clear(&mut self) {
        for bucket in self.buckets.iter_mut() {
            bucket.clear();
        }

        self.size = 0;
    }

    pub fn capacity(&self) -> usize {
        self.buckets.len() * self.bucket_size
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
    use rand::random;

    use super::CuckooFilter;

    #[test]
    fn insert_and_contains_items() {
        let mut filter = CuckooFilter::new(128);

        filter.insert(b"Pi");
        filter.insert(b"Pa");

        assert!(filter.contains(b"Pi"));
        assert!(filter.contains(b"Pa"));
        assert!(!filter.contains(b"Po"));
    }

    #[test]
    fn remove_items() {
        let mut filter = CuckooFilter::new(128);

        filter.insert(b"test");
        assert!(filter.contains(b"test"));

        filter.remove(b"test");
        assert!(!filter.contains(b"test"));
    }

    #[test]
    fn insert_multiple() {
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

        // Matches number of fingerprints in buckets.
        let mut fp_len = 0;
        for bucket in &filter.buckets {
            fp_len += bucket.len();
        }
        assert_eq!(filter.len(), fp_len);
    }

    #[test]
    fn check_len() {
        let mut filter = CuckooFilter::new(128);
        assert_eq!(filter.len(), 0);

        filter.insert(b"a");
        assert_eq!(filter.len(), 1);

        filter.insert(b"b");
        assert_eq!(filter.len(), 2);
    }

    #[test]
    fn uniform_random_items() {
        type Hash = [u8; 32];

        for _ in 0..100 {
            let mut filter = CuckooFilter::<Hash>::new(512);

            let num_items = 64;
            let mut items = Vec::with_capacity(num_items);
            let mut false_items = Vec::with_capacity(num_items);

            let mut failed_inserts = 0;
            let mut failed_contains = 0;

            for _ in 0..num_items {
                let mut item: Hash = [0; 32];
                for i in item.iter_mut() {
                    *i = random();
                }

                if !filter.insert(&item) {
                    failed_inserts += 1;
                }

                items.push(item);
            }

            for item in &items {
                if !filter.contains(item) {
                    failed_contains += 1;
                }
            }

            for _ in 0..num_items {
                let mut item: Hash = [0; 32];
                for i in item.iter_mut() {
                    *i = random();
                }

                false_items.push(item);
            }

            let mut false_positives = 0;
            for item in &false_items {
                if filter.contains(item) {
                    false_positives += 1;
                }
            }

            println!("{}/{}/{}", failed_inserts, failed_contains, false_positives);
        }
    }
}
