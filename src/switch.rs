// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::HashSet;
use std::hash::Hash;
use std::marker::PhantomData;

use crate::cuckoo::{Bitfield, BitfieldError, CuckooFilter, CuckooFilterBuilder};
use crate::hash::{Digest, hash_digest};
use crate::ring::{PushOutcome, RingSet, RingSetMode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message<ID, M> {
    MemoryPage(M),
    RepairRequest(ID, Bitfield),
}

impl<ID, M> From<M> for Message<ID, M> {
    fn from(memory_page: M) -> Self {
        Self::MemoryPage(memory_page)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Outgoing<ID, M> {
    pub updates: Vec<M>,
    pub broadcast: Vec<Message<ID, M>>,
}

impl<ID, M> Default for Outgoing<ID, M> {
    fn default() -> Self {
        Self {
            updates: Vec::new(),
            broadcast: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct MemorySwitchBuilder<ID, M> {
    cache_size: usize,
    history_size: usize,
    filter_capacity: usize,
    _marker: PhantomData<(ID, M)>,
}

impl<ID, M> Default for MemorySwitchBuilder<ID, M> {
    fn default() -> Self {
        Self {
            // The cache_size should be smaller than the filter's capacity (95% max.)
            cache_size: 64,
            // From note: "In practice we dimensioned the blacklist ("history size") to have twice
            // the length of the content cache."
            history_size: 128,
            // With a filter capacity of 128, bucket size of 4, fingerprint bit length 20 we get a
            // 176 bytes bitfield size for 64 items (see cache_size) and 0.01% false-positive rate.
            filter_capacity: 128,
            _marker: PhantomData,
        }
    }
}

impl<ID, M> MemorySwitchBuilder<ID, M>
where
    ID: Copy + Eq + Hash,
    M: Clone + Eq + Hash,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_cache_size(mut self, cache_size: usize) -> Self {
        self.cache_size = cache_size;
        self
    }

    pub fn with_history_size(mut self, history_size: usize) -> Self {
        self.history_size = history_size;
        self
    }

    pub fn with_filter_capacity(mut self, filter_capacity: usize) -> Self {
        self.filter_capacity = filter_capacity;
        self
    }

    pub fn build(self, my_id: ID) -> MemorySwitch<ID, M> {
        MemorySwitch::from_args(
            my_id,
            self.cache_size,
            self.history_size,
            self.filter_capacity,
        )
    }
}

#[derive(Debug)]
pub struct MemorySwitch<ID, M> {
    my_id: ID,
    cache: RingSet<M>,
    history: RingSet<Digest>,
    filter: CuckooFilter<Digest>,
    neighbors: HashSet<ID>,
}

impl<ID, M> MemorySwitch<ID, M>
where
    ID: Copy + Eq + Hash,
    M: Clone + Eq + Hash,
{
    pub fn new(my_id: ID) -> Self {
        MemorySwitchBuilder::default().build(my_id)
    }

    pub fn builder() -> MemorySwitchBuilder<ID, M> {
        MemorySwitchBuilder::new()
    }

    fn from_args(
        my_id: ID,
        cache_size: usize,
        history_size: usize,
        filter_capacity: usize,
    ) -> Self {
        Self {
            my_id,
            cache: RingSet::new(cache_size, RingSetMode::HotToTop),
            history: RingSet::new(history_size, RingSetMode::Regular),
            filter: Self::filter_builder(filter_capacity).build(),
            neighbors: HashSet::with_capacity(16),
        }
    }

    fn filter_builder(filter_capacity: usize) -> CuckooFilterBuilder<Digest> {
        CuckooFilter::builder()
            .with_capacity(filter_capacity)
            .with_bucket_size(4)
            .with_max_evictions(32)
            .with_fingerprint_bits(20)
    }

    pub fn clear(&mut self) {
        self.cache.clear();
        self.history.clear();
        self.filter.clear();
        self.neighbors.clear();
    }

    pub fn clear_neighbors(&mut self) {
        self.neighbors.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn add(&mut self, memory_page: M) -> Outgoing<ID, M> {
        self.on_memory_page(memory_page)
    }

    pub fn incoming(&mut self, message: Message<ID, M>) -> Result<Outgoing<ID, M>, BitfieldError> {
        match message {
            Message::MemoryPage(memory_page) => Ok(self.on_memory_page(memory_page)),
            Message::RepairRequest(id, bitfield) => self.on_repair_request(id, bitfield),
        }
    }

    pub fn slow_repair(&self) -> Outgoing<ID, M> {
        let bitfield = self.filter.bitfield();

        Outgoing {
            updates: vec![],
            broadcast: vec![Message::RepairRequest(self.my_id, bitfield)],
        }
    }

    fn on_repair_request(
        &mut self,
        id: ID,
        bitfield: Bitfield,
    ) -> Result<Outgoing<ID, M>, BitfieldError> {
        if id == self.my_id {
            return Ok(Outgoing::default());
        }
        self.neighbors.insert(id);

        if self.ignore_request() {
            return Ok(Outgoing::default());
        }

        let remote_filter =
            Self::filter_builder(self.filter.capacity()).build_from_bitfield(bitfield)?;

        let mut broadcast = Vec::new();
        for memory_page in self.cache.iter() {
            let hash = hash_digest(&memory_page);

            if !remote_filter.contains(&hash) {
                broadcast.push(memory_page.clone().into());
            }
        }

        Ok(Outgoing {
            updates: vec![],
            broadcast,
        })
    }

    fn ignore_request(&self) -> bool {
        // From MemoryLAN note: "As a first approximation, a reply probability of 1/d is helpful
        // where d is the number of neighbors. In practice, a more agressive dampening (1/(2 * d) or
        // 1/d^2) is more efficient"
        !rand::random_bool(1f64 / std::cmp::max(1, self.neighbors.len()) as f64) // 1/d
    }

    fn on_memory_page(&mut self, memory_page: M) -> Outgoing<ID, M> {
        let hash = hash_digest(&memory_page);

        match self.cache.push(memory_page.clone()) {
            PushOutcome::Evicted(old_memory_page) => {
                let old_hash = hash_digest(&old_memory_page);
                self.filter.remove(&old_hash);
            }
            PushOutcome::Inserted => {
                self.filter.insert(&hash);
            }
            _ => (),
        }

        debug_assert_eq!(
            self.cache.len(),
            self.filter.len(),
            "same items in filter as in cache"
        );

        if self.history.push(hash).was_ignored() {
            return Outgoing::default();
        }

        Outgoing {
            updates: vec![memory_page.clone()],  // Delivery
            broadcast: vec![memory_page.into()], // Flooding
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MemorySwitch;

    #[test]
    fn fast_push_broadcast() {
        let mut switch_1 = MemorySwitch::new("switch-1");

        let outgoing_1 = switch_1.add("Hello, is anybody listening?");
        assert_eq!(outgoing_1.updates.len(), 1);
        assert_eq!(outgoing_1.broadcast.len(), 1);
        assert_eq!(switch_1.len(), 1);

        let mut switch_2 = MemorySwitch::new("switch-2");

        let outgoing_2 = switch_2.incoming(outgoing_1.broadcast[0].clone()).unwrap();
        assert_eq!(outgoing_2.updates.len(), 1);
        assert_eq!(outgoing_2.broadcast.len(), 1);
        assert_eq!(switch_2.len(), 1);
    }

    #[test]
    fn filter_duplicates() {
        let mut switch = MemorySwitch::new("test");

        let outgoing = switch.add("Yet again and again and again");
        assert_eq!(outgoing.updates.len(), 1);
        assert_eq!(outgoing.broadcast.len(), 1);
        assert_eq!(switch.len(), 1);

        let outgoing = switch.add("Yet again and again and again");
        assert_eq!(outgoing.updates.len(), 0);
        assert_eq!(outgoing.broadcast.len(), 0);
        assert_eq!(switch.len(), 1);
    }

    #[test]
    fn slow_repair() {
        let mut switch_1 = MemorySwitch::new("switch-1");

        // 1 broadcasts first message (not received by 2).
        let outgoing_1 = switch_1.add("tick");
        assert_eq!(outgoing_1.broadcast.len(), 1);
        assert_eq!(switch_1.len(), 1);

        // 1 broadcasts repair request.
        let outgoing_1 = switch_1.slow_repair();
        assert_eq!(outgoing_1.updates.len(), 0);
        assert_eq!(outgoing_1.broadcast.len(), 1);

        let mut switch_2 = MemorySwitch::new("switch-2");

        // 2 broadcasts two messages (not received by 1).
        switch_2.add("trick");
        switch_2.add("track");

        // 2 receives repair request of 1.
        let outgoing_2 = switch_2.incoming(outgoing_1.broadcast[0].clone()).unwrap();
        assert_eq!(outgoing_2.updates.len(), 0);
        assert_eq!(outgoing_2.broadcast.len(), 2);
        assert_eq!(switch_2.len(), 2);

        // 1 receives repair responses of 2.
        for message in outgoing_2.broadcast {
            let outgoing_1 = switch_1.incoming(message).unwrap();
            assert_eq!(outgoing_1.updates.len(), 1);
            assert_eq!(outgoing_1.broadcast.len(), 1);
        }

        // 1 should have all messages now.
        assert_eq!(switch_1.len(), 3);
    }
}
