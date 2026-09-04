use std::hash::Hash;

use crate::cuckoo::{BucketIndex, Fingerprint};
use crate::hash::{Digest, hash_digest};

pub fn fingerprint(hash: Digest) -> Fingerprint {
    hash as Fingerprint
}

pub fn fingerprint_index<T: ?Sized + Hash>(
    item: &T,
    num_buckets: usize,
) -> (Fingerprint, BucketIndex) {
    let hash = hash_digest(&item);
    let fp = fingerprint(hash);
    let i1 = hash as BucketIndex & (num_buckets as BucketIndex - 1);
    (fp, i1)
}

pub fn alt_index(fingerprint: Fingerprint, index: BucketIndex, num_buckets: usize) -> BucketIndex {
    let alt_hash = index as Digest ^ hash_digest(&fingerprint);

    alt_hash as BucketIndex & (num_buckets as BucketIndex - 1)
}

#[cfg(test)]
mod tests {
    use crate::hash::Digest;

    use super::{alt_index, fingerprint_index};

    #[test]
    fn modulo_num_buckets() {
        for i in 0..u16::MAX {
            let (_fp, i1) = fingerprint_index(&i, 64);
            assert!(i1 <= 63);
        }
    }

    #[test]
    fn xor_symmetry() {
        let item: [u8; 32] = [0; 32];

        let (fp, i1) = fingerprint_index(&item, 64);

        // h2(x) = h1(x) XOR hash(fp(x))
        let i2 = alt_index(fp, i1, 64);

        // h1(x) = h2(x) XOR hash(fp(x))
        let i3 = alt_index(fp, i2, 64);

        assert_eq!(i1, i3);
    }
}
