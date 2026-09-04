use crate::cuckoo::DEFAULT_BUCKET_SIZE;

pub type Fingerprint = u16;

#[derive(Clone, Debug)]
pub struct Bucket {
    size: usize,
    fingerprints: Vec<Fingerprint>,
}

impl Default for Bucket {
    fn default() -> Self {
        Self::new(DEFAULT_BUCKET_SIZE)
    }
}

impl Bucket {
    pub fn new(bucket_size: usize) -> Self {
        Self {
            size: bucket_size,
            fingerprints: Vec::with_capacity(bucket_size),
        }
    }

    pub fn insert(&mut self, fp: Fingerprint) -> bool {
        if self.fingerprints.len() < self.size {
            self.fingerprints.push(fp);
            true
        } else {
            false
        }
    }

    pub fn force_insert(&mut self, fp: Fingerprint) -> Option<Fingerprint> {
        let result = if self.is_full() {
            let old_fp = self.fingerprints.remove(0);
            self.fingerprints.push(fp);
            Some(old_fp)
        } else {
            None
        };

        self.fingerprints.push(fp);

        result
    }

    pub fn remove(&mut self, fp: Fingerprint) -> bool {
        if let Some(index) = self.fingerprints.iter().position(|&x| x == fp) {
            self.fingerprints.remove(index);
            true
        } else {
            false
        }
    }

    pub fn contains(&self, fp: Fingerprint) -> bool {
        self.fingerprints.contains(&fp)
    }

    pub fn is_full(&self) -> bool {
        self.fingerprints.len() >= self.size
    }

    pub fn clear(&mut self) {
        self.fingerprints.clear()
    }

    pub fn len(&self) -> usize {
        self.fingerprints.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fingerprints.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::Bucket;

    #[test]
    fn insert_with_capacity() {
        let mut bucket = Bucket::default();

        // Insert items and check length.
        assert!(bucket.insert(8));
        assert!(bucket.insert(9));
        assert_eq!(bucket.len(), 2);

        // Bucket size is limited to 4.
        assert!(bucket.insert(1));
        assert!(bucket.insert(2));
        assert!(!bucket.insert(3));
        assert!(!bucket.insert(4));
        assert_eq!(bucket.len(), 4);

        // Clear all items.
        bucket.clear();
        assert_eq!(bucket.len(), 0);
    }

    #[test]
    fn remove_items() {
        let mut bucket = Bucket::default();

        bucket.insert(1);
        assert!(bucket.contains(1));
        bucket.remove(1);
        assert!(!bucket.contains(1));
    }

    #[test]
    fn fifo_order_when_force_insert() {
        let mut bucket = Bucket::default();

        bucket.insert(2);
        bucket.insert(3);
        bucket.insert(4);
        bucket.insert(5);

        assert_eq!(bucket.force_insert(6), Some(2));
        assert!(bucket.remove(3));
        assert!(!bucket.remove(3));
        assert_eq!(bucket.force_insert(7), Some(4));
        assert_eq!(bucket.force_insert(8), Some(5));
        assert_eq!(bucket.force_insert(9), Some(6));
    }
}
