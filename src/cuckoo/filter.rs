use std::hash::Hash;
use std::marker::PhantomData;

use crate::cuckoo::utils::{alt_index, fingerprint_index};
use crate::cuckoo::{
    Bitfield, BitfieldError, Bucket, BucketIndex, DEFAULT_BUCKET_SIZE, DEFAULT_CAPACITY,
    DEFAULT_FINGERPRINT_BITS, DEFAULT_MAX_EVICTIONS, Fingerprint,
};

pub struct CuckooFilterBuilder<T>
where
    T: ?Sized + Hash,
{
    capacity: usize,
    max_evictions: usize,
    bucket_size: usize,
    fp_bits: u32,
    _marker: PhantomData<T>,
}

impl<T> CuckooFilterBuilder<T>
where
    T: ?Sized + Hash,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(mut self, capacity: usize) -> Self {
        if capacity == 0 {
            panic!("capacity can't be zero");
        }

        self.capacity = capacity;
        self
    }

    pub fn with_max_evictions(mut self, max_evictions: usize) -> Self {
        self.max_evictions = max_evictions;
        self
    }

    pub fn with_bucket_size(mut self, bucket_size: usize) -> Self {
        if bucket_size == 0 {
            panic!("bucket size can't be zero");
        } else if bucket_size > 15 {
            // The bitfield encodes the len of a bucket with 4 bits which gives us only a max. of
            // 15 (0b1111 = 15).
            panic!("bucket size can't be larger than 15");
        }

        self.bucket_size = bucket_size;
        self
    }

    pub fn with_fingerprint_bits(mut self, fp_bits: u32) -> Self {
        if fp_bits > Fingerprint::bit_width(Fingerprint::MAX) {
            panic!(
                "fp_bits can't be larger than {}",
                Fingerprint::bit_width(Fingerprint::MAX)
            );
        } else if fp_bits == 0 {
            panic!("fp_bits can't be zero");
        }

        self.fp_bits = fp_bits;
        self
    }

    pub fn build_from_bitfield(self, bitfield: Bitfield) -> Result<CuckooFilter<T>, BitfieldError> {
        CuckooFilter::<T>::from_bitfield(
            bitfield,
            self.capacity,
            self.bucket_size,
            self.max_evictions,
            self.fp_bits,
        )
    }

    pub fn build(self) -> CuckooFilter<T> {
        CuckooFilter::<T>::new(
            self.capacity,
            self.bucket_size,
            self.max_evictions,
            self.fp_bits,
        )
    }
}

impl<T> Default for CuckooFilterBuilder<T>
where
    T: ?Sized + Hash,
{
    fn default() -> Self {
        Self {
            capacity: DEFAULT_CAPACITY,
            max_evictions: DEFAULT_MAX_EVICTIONS,
            bucket_size: DEFAULT_BUCKET_SIZE,
            fp_bits: DEFAULT_FINGERPRINT_BITS,
            _marker: PhantomData,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CuckooFilter<T>
where
    T: ?Sized + Hash,
{
    buckets: Vec<Bucket>,
    size: usize,
    max_evictions: usize,
    bucket_size: usize,
    fp_bits: u32,
    _marker: PhantomData<T>,
}

impl<T> Default for CuckooFilter<T>
where
    T: ?Sized + Hash,
{
    fn default() -> Self {
        CuckooFilterBuilder::default().build()
    }
}

impl<T> CuckooFilter<T>
where
    T: ?Sized + Hash,
{
    pub fn builder() -> CuckooFilterBuilder<T> {
        CuckooFilterBuilder::new()
    }

    fn new(capacity: usize, bucket_size: usize, max_evictions: usize, fp_bits: u32) -> Self {
        let num_buckets = std::cmp::max(1, capacity.next_power_of_two() / bucket_size);
        let buckets = (0..num_buckets).map(|_| Bucket::new(bucket_size)).collect();

        Self {
            buckets,
            size: 0,
            max_evictions,
            bucket_size,
            fp_bits,
            _marker: PhantomData,
        }
    }

    fn from_bitfield(
        bitfield: Bitfield,
        capacity: usize,
        bucket_size: usize,
        max_evictions: usize,
        fp_bits: u32,
    ) -> Result<Self, BitfieldError> {
        let num_buckets = std::cmp::max(1, capacity.next_power_of_two() / bucket_size);
        let (buckets, size) = bitfield.to_buckets(num_buckets, bucket_size, fp_bits)?;

        Ok(Self {
            buckets,
            size,
            max_evictions,
            bucket_size,
            fp_bits,
            _marker: PhantomData,
        })
    }

    pub fn insert(&mut self, item: &T) -> bool {
        let num_buckets = self.buckets.len();

        let (fp, i1) = fingerprint_index(item, num_buckets, self.fp_bits);
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

        let (fp, i1) = fingerprint_index(item, num_buckets, self.fp_bits);
        if self.buckets[i1].contains(fp) {
            return true;
        }

        let i2 = alt_index(fp, i1, num_buckets);
        self.buckets[i2].contains(fp)
    }

    pub fn remove(&mut self, item: &T) -> bool {
        let num_buckets = self.buckets.len();

        let (fp, i1) = fingerprint_index(item, num_buckets, self.fp_bits);
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

    pub fn num_buckets(&self) -> usize {
        self.buckets.len()
    }

    pub fn capacity(&self) -> usize {
        self.buckets.len() * self.bucket_size
    }

    /// Encodes an efficient bitfield from filter.
    pub fn bitfield(&self) -> Bitfield {
        Bitfield::from_buckets(&self.buckets, self.fp_bits)
    }

    /// Returns estimated bitfield length in bytes.
    pub fn bitfield_len(&self) -> usize {
        Bitfield::estimate_len(&self.buckets, self.fp_bits)
    }

    /// Returns maximum bitfield length in bytes when all buckets are full.
    pub fn bitfield_max_len(&self) -> usize {
        Bitfield::estimate_max_len(self.buckets.len(), self.bucket_size, self.fp_bits)
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

    use crate::cuckoo::DEFAULT_CAPACITY;

    use super::CuckooFilter;

    #[test]
    fn insert_and_contains_items() {
        let mut filter = CuckooFilter::default();
        assert_eq!(filter.capacity(), DEFAULT_CAPACITY);

        filter.insert(b"Pi");
        filter.insert(b"Pa");

        assert!(filter.contains(b"Pi"));
        assert!(filter.contains(b"Pa"));
        assert!(!filter.contains(b"Po"));
    }

    #[test]
    fn remove_items() {
        let mut filter = CuckooFilter::default();

        filter.insert(b"test");
        assert!(filter.contains(b"test"));

        filter.remove(b"test");
        assert!(!filter.contains(b"test"));
    }

    #[test]
    fn insert_multiple() {
        let mut filter = CuckooFilter::builder().with_capacity(128).build();
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
        let mut filter = CuckooFilter::default();
        assert_eq!(filter.len(), 0);

        filter.insert(b"a");
        assert_eq!(filter.len(), 1);

        filter.insert(b"b");
        assert_eq!(filter.len(), 2);
    }

    #[test]
    fn uniform_random_items() {
        type Hash = [u8; 32];

        let sample_size = 256;
        let mut false_positive_counts = 0;

        for _ in 0..sample_size {
            let mut filter = CuckooFilter::<Hash>::builder()
                .with_capacity(128)
                .with_fingerprint_bits(20)
                .with_bucket_size(4)
                .with_max_evictions(32)
                .build();

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

                // If this is high, we have too little capacity.
                if !filter.insert(&item) {
                    failed_inserts += 1;
                }

                items.push(item);
            }

            // If this is high, we kicked out too many items due to low evict num or capacity.
            for item in &items {
                if !filter.contains(item) {
                    failed_contains += 1;
                }
            }

            assert_eq!(failed_inserts, failed_contains);

            for _ in 0..num_items {
                let mut item: Hash = [0; 32];
                for i in item.iter_mut() {
                    *i = random();
                }

                false_items.push(item);
            }

            // Fingerprint size can improve this.
            let mut false_positives = 0;
            for item in &false_items {
                if filter.contains(item) {
                    false_positives += 1;
                }
            }

            if false_positives > 0 {
                false_positive_counts += 1;
            }

            assert!(false_positives <= 1);
            assert_eq!(filter.bitfield_len(), 176);
        }

        assert!(false_positive_counts < 10);
    }

    #[test]
    fn from_bitfield() {
        type Hash = [u8; 32];

        let num_items = 64;
        let mut items = Vec::with_capacity(num_items);

        // Create one filter and populate it with random items.
        let mut filter = CuckooFilter::<Hash>::builder()
            .with_capacity(128)
            .with_fingerprint_bits(20)
            .with_bucket_size(4)
            .with_max_evictions(32)
            .build();

        for _ in 0..num_items {
            let mut item: Hash = [0; 32];
            for i in item.iter_mut() {
                *i = random();
            }

            filter.insert(&item);
            items.push(item);
        }

        // Generate a bitfield from filter.
        let bitfield = filter.bitfield();

        // .. and import it into second filter.
        let filter_again = CuckooFilter::<Hash>::builder()
            .with_capacity(128)
            .with_fingerprint_bits(20)
            .with_bucket_size(4)
            .with_max_evictions(32)
            .build_from_bitfield(bitfield)
            .expect("bitfield encoding is correct");

        // They should be the same.
        assert_eq!(filter, filter_again);
        for item in &items {
            assert!(filter_again.contains(item));
        }
    }
}
