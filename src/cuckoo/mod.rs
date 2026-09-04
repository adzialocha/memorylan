mod bitfield;
mod bucket;
mod filter;
mod utils;

pub type Fingerprint = u32;

const DEFAULT_CAPACITY: usize = 128;

const DEFAULT_MAX_EVICTIONS: usize = 32;

const DEFAULT_BUCKET_SIZE: usize = 4;

const DEFAULT_FINGERPRINT_BITS: u32 = 20; // Max. 32

pub type BucketIndex = usize;

pub use bitfield::{Bitfield, BitfieldError};
pub use bucket::Bucket;
pub use filter::{CuckooFilter, CuckooFilterBuilder};
