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

    pub fn push_single_bit(&mut self, bit: u8) {
        self.current |= (bit & 1) << self.filled;
        self.filled += 1;
        self.try_flush();
    }

    pub fn push_bits(&mut self, mut value: u32, mut bits: u32) {
        while bits > 0 {
            let space = 8 - self.filled as u32;
            let take = if bits < space { bits } else { space };

            // Take "take" LSBs from value.
            let mask = if take == 32 {
                u32::MAX
            } else {
                (1u32 << take) - 1
            };

            let chunk = (value & mask) as u8;

            // Place chunk into "current" at position "filled".
            self.current |= chunk << self.filled;
            self.filled += take as u8;
            value >>= take;
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

#[derive(Clone, Debug)]
pub struct Bitfield(Vec<u8>);

impl Bitfield {
    pub(crate) fn from_buckets(buckets: &[Bucket], fp_bits: u32) -> Self {
        let mut packer = BitPacker::new();

        for bucket in buckets {
            for &fp in bucket.fingerprints() {
                let value = if fp_bits == 32 {
                    fp
                } else {
                    fp & ((1u32 << fp_bits) - 1)
                };

                packer.push_bits(value, fp_bits);
            }

            // Divide buckets with a zero-bit.
            packer.push_single_bit(0);
        }

        let bitfield = packer.finalize();

        Self(bitfield)
    }
}

#[cfg(test)]
mod tests {
    use crate::cuckoo::Bucket;

    use super::{BitPacker, Bitfield};

    #[test]
    fn push_single_bit() {
        let mut packer = BitPacker::new();

        // 1011 1000 = 184
        packer.push_single_bit(0x0);
        packer.push_single_bit(0x0);
        packer.push_single_bit(0x0);
        packer.push_single_bit(0x1);
        packer.push_single_bit(0x1);
        packer.push_single_bit(0x1);
        packer.push_single_bit(0x0);
        packer.push_single_bit(0x1);

        // 0000 1101 = 13
        packer.push_single_bit(0x1);
        packer.push_single_bit(0x0);
        packer.push_single_bit(0x1);
        packer.push_single_bit(0x1);

        let output = packer.finalize();
        assert_eq!(output[0], 0b1011_1000);
        assert_eq!(output[1], 0b0000_1101);
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

        assert_eq!(output[0], 0b0101_1110);
        assert_eq!(output[1], 0b0000_1011);
        assert_eq!(output.len(), 2);
    }

    #[test]
    fn push_large_bits() {
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

        assert_eq!(output[0], 0b0101_1110);
        assert_eq!(output[1], 0b0000_1011);
        assert_eq!(output.len(), 2);
    }

    #[test]
    fn bucket_separator() {
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

        assert_eq!(bitfield.0[0], 0b0010_0001);
        assert_eq!(bitfield.0[1], 0b1000_0011);
        //                             ^ Separator
        assert_eq!(bitfield.0[2], 0b0000_0000);
        //                                 ^ Separator
    }
}
