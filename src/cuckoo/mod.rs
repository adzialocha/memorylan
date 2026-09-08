mod bitfield;
mod bucket;
mod filter;
mod utils;

pub type Fingerprint = u32;

pub type BucketIndex = usize;

pub use bitfield::{Bitfield, BitfieldError};
pub use bucket::Bucket;
pub use filter::{CuckooFilter, CuckooFilterBuilder};
