use std::hash::Hash;

use crate::cuckoo::Bitfield;
use crate::hash::{Digest, hash_digest};
use crate::ring::{RingSet, RingSetMode};

pub enum Message<M> {
    MemoryPage(M),
    RepairRequest(Bitfield),
}

pub struct MemoryLan<M> {
    cache: RingSet<M>,
    history: RingSet<Digest>,
}

impl<M> Default for MemoryLan<M>
where
    M: Clone + Eq + Hash,
{
    fn default() -> Self {
        Self::new(32, 64)
    }
}

impl<M> MemoryLan<M>
where
    M: Clone + Eq + Hash,
{
    pub fn new(cache_size: usize, history_size: usize) -> Self {
        Self {
            cache: RingSet::new(cache_size, RingSetMode::HotToTop),
            history: RingSet::new(history_size, RingSetMode::Regular),
        }
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.history.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn process(&mut self, message: Message<M>) {
        match message {
            Message::MemoryPage(memory_page) => self.incoming_memory_page(memory_page),
            Message::RepairRequest(bitfield) => {
                self.incoming_repair_request(bitfield);
            }
        }
    }

    pub fn request_slow_repair(&self) {
        todo!();
    }

    fn incoming_repair_request(&mut self, _bitfield: Bitfield) {
        todo!();
    }

    fn incoming_memory_page(&mut self, memory_page: M) {
        let hash = hash_digest(&memory_page);

        self.cache.push(memory_page);

        if self.history.push(hash).is_none() {
            return;
        }

        todo!();
    }
}
