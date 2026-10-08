//! Gateway lookup rejects guest code outside the registered address envelope.
//! Bounds follow every insertion, including arbitrary addresses installed by
//! the loader. The ordered map remains authoritative for hits and holes.
use std::{collections::BTreeMap, ops::Index};

pub struct Gateways {
    entries: BTreeMap<u64, String>,
    first: u64,
    last: u64,
}

impl From<BTreeMap<u64, String>> for Gateways {
    fn from(entries: BTreeMap<u64, String>) -> Self {
        Self {
            first: entries.first_key_value().map_or(u64::MAX, |(&at, _)| at),
            last: entries.last_key_value().map_or(0, |(&at, _)| at),
            entries,
        }
    }
}

impl Gateways {
    pub fn get(&self, address: &u64) -> Option<&String> {
        if *address < self.first || *address > self.last {
            return None;
        }
        self.entries.get(address)
    }

    pub fn contains_key(&self, address: &u64) -> bool {
        self.get(address).is_some()
    }

    pub fn insert(&mut self, address: u64, name: String) -> Option<String> {
        self.first = self.first.min(address);
        self.last = self.last.max(address);
        self.entries.insert(address, name)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> std::collections::btree_map::Iter<'_, u64, String> {
        self.entries.iter()
    }

    pub fn values(&self) -> std::collections::btree_map::Values<'_, u64, String> {
        self.entries.values()
    }
}

impl Index<&u64> for Gateways {
    type Output = String;

    fn index(&self, address: &u64) -> &Self::Output {
        &self.entries[address]
    }
}
