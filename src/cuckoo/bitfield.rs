use crate::cuckoo::Bucket;

#[derive(Debug, Default)]
struct BitPacker {
    output: Vec<u8>,
    current: u8,
    filled: u8,
}

impl BitPacker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            output: Vec::with_capacity(capacity),
            ..Default::default()
        }
    }

    pub fn push_single_bit(&mut self, bit: u8) {
        // First bit goes to bit 7 (MSB) of the byte.
        self.current |= (bit & 1) << (7 - self.filled);
        self.filled += 1;
        self.try_flush();
    }

    pub fn push_bits(&mut self, mut value: u32, mut bits: u32) {
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

// This assumes that bucket size will never be larger than 15 (0b1111 = 15).
const BUCKET_PREFIX_LEN: u32 = 4;

#[derive(Clone, Debug)]
pub struct Bitfield(Vec<u8>);

impl Bitfield {
    /// Returns estimated bitfield length in bytes.
    pub fn estimate_len(buckets: &[Bucket], fp_bits: u32) -> usize {
        let mut result = 0;
        for bucket in buckets {
            result += BUCKET_PREFIX_LEN as usize; // Length prefix for each bucket.
            result += bucket.len() * fp_bits as usize;
        }
        result.div_ceil(8) // in bytes.
    }

    /// Returns maximum bitfield length in bytes when all buckets are full.
    pub fn estimate_max_len(num_buckets: usize, bucket_size: usize, fp_bits: u32) -> usize {
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
            packer.push_bits(fingerprints.len() as u32, BUCKET_PREFIX_LEN);

            for &fp in fingerprints {
                packer.push_bits(fp, fp_bits);
            }
        }

        Self(packer.finalize())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

#[cfg(test)]
mod tests {
    use crate::cuckoo::Bucket;

    use super::{BitPacker, Bitfield};

    #[test]
    fn push_single_bit() {
        let mut packer = BitPacker::new();

        // 0001 1101
        packer.push_single_bit(0x0);
        packer.push_single_bit(0x0);
        packer.push_single_bit(0x0);
        packer.push_single_bit(0x1);
        packer.push_single_bit(0x1);
        packer.push_single_bit(0x1);
        packer.push_single_bit(0x0);
        packer.push_single_bit(0x1);

        // 1101 0000
        packer.push_single_bit(0x1);
        packer.push_single_bit(0x0);
        packer.push_single_bit(0x1);
        packer.push_single_bit(0x1);

        let output = packer.finalize();
        assert_eq!(output[0], 0b0001_1101);
        assert_eq!(output[1], 0b1011_0000);
        assert_eq!(output.len(), 2);
    }

    #[test]
    fn push_bits() {
        let mut packer = BitPacker::new();

        // 110
        packer.push_bits(0b0001_1110, 3);

        // 011
        packer.push_bits(0b1101_0011, 3);

        // 101
        packer.push_bits(0b0001_1101, 3);

        // 101
        packer.push_single_bit(0x1);
        packer.push_single_bit(0x0);
        packer.push_single_bit(0x1);

        let output = packer.finalize();
        assert_eq!(output[0], 0b1100_1110);
        assert_eq!(output[1], 0b1101_0000);
        assert_eq!(output.len(), 2);
    }

    #[test]
    fn push_large_bits() {
        let mut packer = BitPacker::new();
        packer.push_bits(0xFFFF_FFFF, 20);

        let output = packer.finalize();
        assert_eq!(output[0], 0b1111_1111);
        assert_eq!(output[1], 0b1111_1111);
        assert_eq!(output[2], 0b1111_0000);
        assert_eq!(output.len(), 3);
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
}
