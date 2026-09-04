use std::hash::{DefaultHasher, Hash, Hasher};

pub type Digest = u64;

pub fn hash_digest<T: Hash>(item: &T) -> Digest {
    let mut state = DefaultHasher::new(); // SipHasher 1-3
    item.hash(&mut state);
    state.finish()
}
