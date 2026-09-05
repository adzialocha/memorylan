use crate::cuckoo::Bucket;

#[derive(Debug, Default)]
struct BitPacker {
    output: Vec<u8>,
    current: u8,
    filled: u8,
}

impl BitPacker {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            output: Vec::with_capacity(capacity),
            ..Default::default()
        }
    }

    pub fn push(&mut self, mut value: u32, mut bits: u32) {
        while bits > 0 {
            let space = 8 - self.filled as u32;
            let take = if bits < space { bits } else { space };

            let mask = if take == 32 {
                u32::MAX
            } else {
                (1u32 << take) - 1
            };

            let shift = bits - take;
            let chunk = ((value >> shift) & mask) as u8;

            let dest_shift = (8 - self.filled as u32 - take) as u8;
            self.current |= chunk << dest_shift;
            self.filled += take as u8;
            value &= if shift == 0 { 0 } else { (1u32 << shift) - 1 };
            bits -= take;

            self.try_flush();
        }
    }

    pub fn try_flush(&mut self) {
        if self.filled == 8 {
            self.output.push(self.current);
            self.current = 0;
            self.filled = 0;
        }
    }

    pub fn finalize(mut self) -> Vec<u8> {
        // Flush last byte if needed.
        if self.filled > 0 {
            self.output.push(self.current);
        }

        self.output
    }
}

#[derive(Debug)]
struct BitUnpacker<'a> {
    data: &'a [u8],
    byte_index: usize,
    bit_position: u8,
}

impl<'a> BitUnpacker<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_index: 0,
            bit_position: 0,
        }
    }

    /// Returns how many bits remain available.
    fn bits_left(&self) -> u32 {
        let left = self.data.len().saturating_sub(self.byte_index) as u32;
        left.saturating_mul(8)
            .saturating_sub(self.bit_position as u32)
    }

    /// Read 1 to 32 bits MSB-first and return them as u32. Returns None if there not enough bits
    /// remaining.
    pub fn read_bits(&mut self, mut bits: u32) -> Option<u32> {
        if !(1..=32).contains(&bits) {
            panic!("can only read 1-32 bits");
        }

        // Trying to read more bits than available.
        if self.bits_left() < bits {
            return None;
        }

        // We always read per byte, up to 4 of them aka 32 bits. We use the bit_position cursor to
        // track how far we've read the current byte (tracked by byte_index).
        //
        //        byte_index
        //             v
        // 0          -1---------  2           3
        // 1010 1010 | 1001 0010 | 1100 0101 | 1001 1110
        //                  ^
        //               bit_position
        //
        // The final value is returned as an u32.
        let mut value: u32 = 0;

        while bits > 0 {
            // How many bits are available in the current byte?
            let available = 8u32 - self.bit_position as u32;
            let take = if bits < available { bits } else { available };

            // Read next chunk.
            let byte = self.data[self.byte_index];

            let shift_in_byte = available - take;
            let mask = if take == 8 {
                0xFFu8
            } else {
                ((1u8 << take) - 1) << shift_in_byte
            };
            let chunk = ((byte & mask) >> shift_in_byte) as u32;

            // Append chunk to value (MSB-first).
            value = (value << take) | chunk;

            // Consume bits and move cursors.
            self.bit_position += take as u8;
            if self.bit_position == 8 {
                self.byte_index += 1;
                self.bit_position = 0;
            }
            bits -= take;
        }

        Some(value)
    }
}

// This assumes that bucket size will never be larger than 15 (0b1111 = 15).
const BUCKET_PREFIX_LEN: u32 = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(transparent)
)]
pub struct Bitfield(#[cfg_attr(feature = "serde", serde(with = "serde_bytes"))] Vec<u8>);

impl Bitfield {
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Self {
        let bytes = bytes.as_ref();
        Self(bytes.to_vec())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns estimated bitfield length in bytes.
    pub(crate) fn estimate_len(buckets: &[Bucket], fp_bits: u32) -> usize {
        let mut result = 0;
        for bucket in buckets {
            result += BUCKET_PREFIX_LEN as usize; // Length prefix for each bucket.
            result += bucket.len() * fp_bits as usize;
        }
        result.div_ceil(8) // in bytes.
    }

    /// Returns maximum bitfield length in bytes when all buckets are full.
    #[allow(unused)]
    pub(crate) fn estimate_max_len(num_buckets: usize, bucket_size: usize, fp_bits: u32) -> usize {
        let result = (num_buckets * BUCKET_PREFIX_LEN as usize)
            + (num_buckets * bucket_size * fp_bits as usize);
        result.div_ceil(8) // in bytes.
    }

    /// Encodes an efficient bitfield from cuckoo-filter buckets.
    pub(crate) fn from_buckets(buckets: &[Bucket], fp_bits: u32) -> Self {
        let capacity = Self::estimate_len(buckets, fp_bits);
        let mut packer = BitPacker::with_capacity(capacity);

        for bucket in buckets {
            let fingerprints = bucket.fingerprints();

            // 4-bit length prefix for bucket.
            packer.push(fingerprints.len() as u32, BUCKET_PREFIX_LEN);

            for &fp in fingerprints {
                packer.push(fp, fp_bits);
            }
        }

        Self(packer.finalize())
    }

    /// Parse expected number of buckets from this bitfield and return error otherwise.
    pub(crate) fn to_buckets(
        &self,
        num_buckets: usize,
        bucket_size: usize,
        fp_bits: u32,
    ) -> Result<(Vec<Bucket>, usize), BitfieldError> {
        let mut unpacker = BitUnpacker::new(&self.0);
        let mut buckets: Vec<Bucket> = Vec::with_capacity(num_buckets);
        let mut size = 0;

        for _ in 0..num_buckets {
            let mut bucket = Bucket::new(bucket_size);

            // Read length prefix for bucket.
            let len = match unpacker.read_bits(BUCKET_PREFIX_LEN) {
                Some(prefix) => prefix as usize,
                None => return Err(BitfieldError::LenPrefixMissing),
            };

            if len > bucket_size {
                return Err(BitfieldError::ExceededBucketSize);
            }

            // Read fingerprints for bucket.
            for _ in 0..len {
                match unpacker.read_bits(fp_bits) {
                    Some(fp) => {
                        if bucket.len() >= len {
                            return Err(BitfieldError::ExceededLenPrefix);
                        }

                        size += 1;
                        bucket.insert(fp);
                    }
                    None => return Err(BitfieldError::FingerprintMissing),
                }
            }

            buckets.push(bucket);
        }

        Ok((buckets, size))
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

#[derive(Debug)]
pub enum BitfieldError {
    /// Not enough bits left in data to read expected bucket len prefix.
    LenPrefixMissing,

    /// Not enough bits left to read expected fingerprint.
    FingerprintMissing,

    /// Number of fingerprints exceeded what was written in bucket len prefix.
    ExceededLenPrefix,

    /// Bucket len prefix is larger than expected bucket size.
    ExceededBucketSize,
}

impl std::fmt::Display for BitfieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let description = match self {
            Self::LenPrefixMissing => "not enough bits left to read expected bucket len prefix",
            Self::FingerprintMissing => "not enough bits left to read expected fingerprint",
            Self::ExceededLenPrefix => "exceeded prefix len when reading fingerprints",
            Self::ExceededBucketSize => "len prefix exceeded bucket size",
        };

        write!(f, "{}", description)
    }
}

impl std::error::Error for BitfieldError {}

#[cfg(test)]
mod tests {
    use rand::{random, random_range};

    use crate::cuckoo::Bucket;
    use crate::cuckoo::utils::fingerprint;
    use crate::hash::hash_digest;

    use super::{BitPacker, BitUnpacker, Bitfield, BitfieldError};

    #[test]
    fn pack_values() {
        let mut packer = BitPacker::default();

        // 0001 1101
        packer.push(0x0, 1);
        packer.push(0x0, 1);
        packer.push(0x0, 1);
        packer.push(0x1, 1);
        packer.push(0x1, 1);
        packer.push(0x1, 1);
        packer.push(0x0, 1);
        packer.push(0x1, 1);

        // 1101
        packer.push(0b1101, 4);

        let output = packer.finalize();
        assert_eq!(output[0], 0b0001_1101);
        assert_eq!(output[1], 0b1101_0000);
        assert_eq!(output.len(), 2);
    }

    #[test]
    fn pack_values_exceeding_single_byte() {
        let mut packer = BitPacker::default();
        packer.push(0xFFFF_FFFF, 20);

        let output = packer.finalize();
        assert_eq!(output[0], 0b1111_1111);
        assert_eq!(output[1], 0b1111_1111);
        assert_eq!(output[2], 0b1111_0000);
        assert_eq!(output.len(), 3);
    }

    #[test]
    fn unpack() {
        let mut unpacker = BitUnpacker::new(&[0b0011_1101, 0b0110_1111]);
        assert_eq!(unpacker.bits_left(), 16);

        assert_eq!(unpacker.read_bits(4), Some(0b0011));
        assert_eq!(unpacker.bits_left(), 12);

        assert_eq!(unpacker.read_bits(4), Some(0b1101));
        assert_eq!(unpacker.bits_left(), 8);

        assert_eq!(unpacker.read_bits(8), Some(0b0110_1111));
        assert_eq!(unpacker.bits_left(), 0);

        assert_eq!(unpacker.read_bits(2), None);
    }

    #[test]
    fn bucket_len_prefix() {
        let fp_bits = 4;

        let fp_1 = 0x1; // b0001
        let fp_2 = 0x2; // b0010
        let fp_3 = 0x3; // b0011
        let fp_4 = 0x4; // b0100

        let mut bucket_1 = Bucket::default();
        bucket_1.insert(fp_1);
        bucket_1.insert(fp_2);
        bucket_1.insert(fp_3);

        let mut bucket_2 = Bucket::default();
        bucket_2.insert(fp_4);

        let bitfield = Bitfield::from_buckets(&[bucket_1, bucket_2], fp_bits);

        assert_eq!(bitfield.0[0], 0b0011_0001);
        //                          ^^^^ Prefix (len=3)
        assert_eq!(bitfield.0[1], 0b0010_0011);
        assert_eq!(bitfield.0[2], 0b0001_0100);
        //                          ^^^^ Prefix (len=1)
        assert_eq!(bitfield.0.len(), 3);
    }

    #[test]
    fn bucket_roundtrip() {
        let fp_bits = 7;
        let num_buckets = 32;
        let bucket_size = 6;

        let mut buckets = Vec::with_capacity(num_buckets);
        let mut size = 0;

        for _ in 0..num_buckets {
            let mut bucket = Bucket::new(bucket_size);

            for _ in 0..random_range(0..bucket_size + 1) {
                let mut item: [u8; 32] = [0; 32];
                for i in item.iter_mut() {
                    *i = random();
                }

                let hash = hash_digest(&item);
                let fp = fingerprint(hash, fp_bits);

                bucket.insert(fp);
                size += 1;
            }

            buckets.push(bucket);
        }

        let bitfield = Bitfield::from_buckets(&buckets, fp_bits);
        let (buckets_again, size_again) = bitfield
            .to_buckets(num_buckets, bucket_size, fp_bits)
            .expect("encoding is correct");

        assert_eq!(buckets, buckets_again);
        assert_eq!(size, size_again);
    }

    #[test]
    fn unpack_not_enough_buckets() {
        // We can store 2 bucket prefixes in one byte (2 x 4 bits) and only have one byte (empty
        // lenghts), but we expect 3 buckets here:
        std::assert_matches!(
            Bitfield(vec![0b0000_0000u8]).to_buckets(3, 4, 8),
            Err(BitfieldError::LenPrefixMissing)
        );
    }

    #[test]
    fn unpack_not_enough_fingerprints() {
        std::assert_matches!(
            Bitfield(vec![0b0110_1001u8, 0b0011_1010u8])
                //          ^^^^ indicate len of 6, but only 3 fingerprints are given.
                .to_buckets(1, 6, 4),
            Err(BitfieldError::FingerprintMissing)
        );
    }

    #[test]
    fn unpack_exceeded_prefix_len() {
        std::assert_matches!(
            Bitfield(vec![0b0010_1100u8]).to_buckets(1, 4, 4),
            //              ^^^^ prefix indicates 2 fingerprints but only one is given.
            Err(BitfieldError::FingerprintMissing)
        );
    }

    #[test]
    fn unpack_exceeded_bucket_size() {
        std::assert_matches!(
            Bitfield(vec![0b1111_0000u8]).to_buckets(1, 10, 8),
            //              ^^^^ indicates a bucket len of 15 but bucket_size is set to 10.
            Err(BitfieldError::ExceededBucketSize)
        );
    }

    #[test]
    fn serde() {
        let bitfield = Bitfield(vec![0b0011_1100, 0b0011_1010]);
        let bytes = postcard::to_allocvec(&bitfield).unwrap();
        let bitfield_again = postcard::from_bytes(&bytes).unwrap();

        assert_eq!(bitfield, bitfield_again);
    }
}
