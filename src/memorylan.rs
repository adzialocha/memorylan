use std::collections::HashSet;
use std::hash::Hash;
use std::marker::PhantomData;

use crate::cuckoo::{Bitfield, BitfieldError, CuckooFilter, CuckooFilterBuilder};
use crate::hash::{Digest, hash_digest};
use crate::ring::{PushOutcome, RingSet, RingSetMode};

const DEFAULT_CACHE_SIZE: usize = 64;

const DEFAULT_HISTORY_SIZE: usize = 128;

const DEFAULT_FILTER_CAPACITY: usize = 128;

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
pub struct MemoryLanBuilder<ID, M> {
    cache_size: usize,
    history_size: usize,
    filter_capacity: usize,
    _marker: PhantomData<(ID, M)>,
}

impl<ID, M> Default for MemoryLanBuilder<ID, M> {
    fn default() -> Self {
        Self {
            cache_size: DEFAULT_CACHE_SIZE,
            history_size: DEFAULT_HISTORY_SIZE,
            filter_capacity: DEFAULT_FILTER_CAPACITY,
            _marker: PhantomData,
        }
    }
}

impl<ID, M> MemoryLanBuilder<ID, M>
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

    pub fn build(self, my_id: ID) -> MemoryLan<ID, M> {
        MemoryLan::new(
            my_id,
            self.cache_size,
            self.history_size,
            self.filter_capacity,
        )
    }
}

#[derive(Debug)]
pub struct MemoryLan<ID, M> {
    my_id: ID,
    cache: RingSet<M>,
    history: RingSet<Digest>,
    filter: CuckooFilter<Digest>,
    neighbors: HashSet<ID>,
}

impl<ID, M> MemoryLan<ID, M>
where
    ID: Copy + Eq + Hash,
    M: Clone + Eq + Hash,
{
    fn new(my_id: ID, cache_size: usize, history_size: usize, filter_capacity: usize) -> Self {
        Self {
            my_id,
            cache: RingSet::new(cache_size, RingSetMode::HotToTop),
            history: RingSet::new(history_size, RingSetMode::Regular),
            filter: Self::filter_builder(filter_capacity).build(),
            neighbors: HashSet::with_capacity(8),
        }
    }

    fn filter_builder(filter_capacity: usize) -> CuckooFilterBuilder<Digest> {
        CuckooFilter::builder()
            .with_capacity(filter_capacity)
            .with_bucket_size(4)
            .with_max_evictions(32)
            .with_fingerprint_bits(20)
    }

    pub fn builder() -> MemoryLanBuilder<ID, M> {
        MemoryLanBuilder::new()
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

        let remote_filter =
            Self::filter_builder(self.filter.capacity()).build_from_bitfield(bitfield)?;

        let mut broadcast = Vec::new();
        for memory_page in self.cache.iter() {
            let hash = hash_digest(&memory_page);

            if !remote_filter.contains(&hash) {
                broadcast.push(memory_page.clone().into());
            }
        }

        if self.ignore_request() {
            return Ok(Outgoing::default());
        }

        Ok(Outgoing {
            updates: vec![],
            broadcast,
        })
    }

    fn ignore_request(&self) -> bool {
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
    use super::MemoryLan;

    #[test]
    fn fast_push_broadcast() {
        let mut lan_1 = MemoryLan::<_, &'static str>::builder().build("node-1");

        let outgoing_1 = lan_1.add("Hello, is anybody listening?");
        assert_eq!(outgoing_1.updates.len(), 1);
        assert_eq!(outgoing_1.broadcast.len(), 1);
        assert_eq!(lan_1.len(), 1);

        let mut lan_2 = MemoryLan::<_, &'static str>::builder().build("node-2");

        let outgoing_2 = lan_2.incoming(outgoing_1.broadcast[0].clone()).unwrap();
        assert_eq!(outgoing_2.updates.len(), 1);
        assert_eq!(outgoing_2.broadcast.len(), 1);
        assert_eq!(lan_2.len(), 1);
    }

    #[test]
    fn filter_duplicates() {
        let mut lan = MemoryLan::<_, &'static str>::builder().build("test");

        let outgoing = lan.add("Yet again and again and again");
        assert_eq!(outgoing.updates.len(), 1);
        assert_eq!(outgoing.broadcast.len(), 1);
        assert_eq!(lan.len(), 1);

        let outgoing = lan.add("Yet again and again and again");
        assert_eq!(outgoing.updates.len(), 0);
        assert_eq!(outgoing.broadcast.len(), 0);
        assert_eq!(lan.len(), 1);
    }

    #[test]
    fn slow_repair() {
        let mut lan_1 = MemoryLan::<_, &'static str>::builder().build("node-1");

        // 1 broadcasts first message (not received by 2).
        let outgoing_1 = lan_1.add("tick");
        assert_eq!(outgoing_1.broadcast.len(), 1);
        assert_eq!(lan_1.len(), 1);

        // 1 broadcasts repair request.
        let outgoing_1 = lan_1.slow_repair();
        assert_eq!(outgoing_1.updates.len(), 0);
        assert_eq!(outgoing_1.broadcast.len(), 1);

        let mut lan_2 = MemoryLan::<_, &'static str>::builder().build("node-2");

        // 2 broadcasts two messages (not received by 1).
        lan_2.add("trick");
        lan_2.add("track");

        // 2 receives repair request of 1.
        let outgoing_2 = lan_2.incoming(outgoing_1.broadcast[0].clone()).unwrap();
        assert_eq!(outgoing_2.updates.len(), 0);
        assert_eq!(outgoing_2.broadcast.len(), 2);
        assert_eq!(lan_2.len(), 2);

        // 1 receives repair responses of 2.
        for message in outgoing_2.broadcast {
            let outgoing_1 = lan_1.incoming(message).unwrap();
            assert_eq!(outgoing_1.updates.len(), 1);
            assert_eq!(outgoing_1.broadcast.len(), 1);
        }

        // 1 should have all messages now.
        assert_eq!(lan_1.len(), 3);
    }
}
