mod bucket;
mod filter;
mod utils;

const DEFAULT_MAX_EVICTIONS: usize = 64;

const DEFAULT_BUCKET_SIZE: usize = 4;

pub type BucketIndex = usize;

pub use bucket::{Bucket, Fingerprint};
// pub use filter::CuckooFilter;
