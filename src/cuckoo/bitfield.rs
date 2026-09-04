use crate::cuckoo::Bucket;

#[derive(Clone, Debug)]
pub struct Bitfield(Vec<u8>);

impl Bitfield {
    pub(crate) fn from_buckets(buckets: &[Bucket], fp_bits: u32) -> Self {
        let mut bitfield = Vec::new();

        for bucket in buckets {
            for fp in bucket.fingerprints() {}
        }

        Self(bitfield)
    }
}

#[cfg(test)]
mod tests {
    use crate::cuckoo::Bucket;
    use crate::cuckoo::utils::fingerprint;

    use super::Bitfield;

    #[test]
    fn generate_from_bucket() {
        let fp_bits = 20;
        let mut bucket = Bucket::default();

        bucket.insert(fingerprint(1, fp_bits));
        bucket.insert(fingerprint(2, fp_bits));
        bucket.insert(fingerprint(3, fp_bits));

        let bitfield = Bitfield::from_buckets(&[bucket], fp_bits);

        println!("{:?}", bitfield);
    }
}
