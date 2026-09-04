mod bucket;
mod filter;
mod utils;

const DEFAULT_CAPACITY: usize = 128;

const DEFAULT_MAX_EVICTIONS: usize = 32;

const DEFAULT_BUCKET_SIZE: usize = 4;

const DEFAULT_FINGERPRINT_BITS: u32 = 16; // Max. 16

pub type BucketIndex = usize;

pub use bucket::{Bucket, Fingerprint};
pub use filter::{CuckooFilter, CuckooFilterBuilder};
