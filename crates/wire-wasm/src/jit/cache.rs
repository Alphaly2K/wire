//! Bounded emitted modules and their entry aliases. Pinned installations and
//! calls stay charged until their final release, even after source revocation.
use super::{Compiled, Memory};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
};

#[cfg(test)]
mod tests;

pub type ModuleId = u64;
pub const DEFAULT_CODE_LIMIT: usize = 64 * 1024 * 1024;
const RETIRED_SLOTS: usize = 64;

thread_local! { static NEXT_ID: Cell<ModuleId> = const { Cell::new(1) }; }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheError {
    Capacity,
    IdentityExhausted,
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Capacity => "Wasm module budget exhausted",
            Self::IdentityExhausted => "Wasm module identity exhausted",
        })
    }
}
impl std::error::Error for CacheError {}

struct Entry {
    code: Compiled,
    charge: usize,
    stamp: u64,
    pins: u32,
    retired: bool,
    slot: u32,
}

pub struct CodeCache {
    modules: BTreeMap<ModuleId, Entry>,
    by_pc: BTreeMap<u64, ModuleId>,
    lru: BTreeSet<(u64, ModuleId)>,
    limit: usize,
    charged: usize,
    pinned_bytes: usize,
    pins: usize,
    clock: u64,
    links: Box<super::links::LinkCache>,
    retired_ids: [u64; RETIRED_SLOTS + 1],
    retired_len: usize,
    retired_overflow: bool,
}

impl Default for CodeCache {
    fn default() -> Self {
        Self::new(DEFAULT_CODE_LIMIT)
    }
}

impl CodeCache {
    pub fn new(limit: usize) -> Self {
        Self {
            modules: BTreeMap::new(),
            by_pc: BTreeMap::new(),
            lru: BTreeSet::new(),
            limit,
            charged: 0,
            pinned_bytes: 0,
            pins: 0,
            clock: 0,
            links: Box::default(),
            retired_ids: [0; RETIRED_SLOTS + 1],
            retired_len: 0,
            retired_overflow: false,
        }
    }

    pub fn charged_bytes(&self) -> usize {
        self.charged
    }
    pub fn limit(&self) -> usize {
        self.limit
    }
    pub fn len(&self) -> usize {
        self.modules.len()
    }
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
    pub fn has_pins(&self) -> bool {
        self.pins != 0
    }
    /// A saturated epoch requires the host to check its retained instances on
    /// every synchronization; an old reclamation tag is never reused.
    pub fn epoch(&self) -> u64 {
        self.links.epoch.get()
    }
    pub fn code(&self, id: ModuleId) -> Option<&Compiled> {
        self.modules.get(&id).map(|entry| &entry.code)
    }

    fn changed(&mut self) {
        self.links.epoch.set(self.epoch().saturating_add(1));
    }

    fn retired(&mut self, id: ModuleId) {
        if self.retired_len == RETIRED_SLOTS {
            self.retired_overflow = true;
        } else {
            self.retired_len += 1;
            self.retired_ids[self.retired_len] = id;
        }
    }

    /// The first word is a count, or MAX when the host must check all records.
    /// The view expires on the next cache mutation or drain.
    pub fn take_retired(&mut self, memory: &Memory) -> &[u64] {
        // Keep source-proof checks in the core, including identities that the
        // host has not finished installing. Retired pinned storage stays charged.
        self.prune_invalid(memory);
        self.retired_ids[0] = if self.retired_overflow || self.epoch() == u64::MAX {
            u64::MAX
        } else {
            self.retired_len as u64
        };
        self.retired_len = 0;
        self.retired_overflow = false;
        &self.retired_ids
    }

    pub fn links(&self) -> &super::links::LinkCache {
        &self.links
    }

    pub fn slot(&self, id: ModuleId) -> u32 {
        self.modules.get(&id).map_or(0, |entry| entry.slot)
    }

    pub fn publish(&mut self, memory: &Memory, id: ModuleId, slot: u32) -> bool {
        if slot == 0 || !self.live(memory, id) {
            return false;
        }
        let entry = self.modules.get_mut(&id).unwrap();
        let changed = entry.slot != 0 && entry.slot != slot;
        entry.slot = slot;
        if changed {
            self.changed();
        }
        true
    }

    fn stamp(&mut self) -> u64 {
        if self.clock == u64::MAX {
            let mut order: Vec<_> = self
                .modules
                .iter()
                .map(|(&id, entry)| (entry.stamp, id))
                .collect();
            order.sort_unstable();
            self.lru.clear();
            for (index, (_, id)) in order.into_iter().enumerate() {
                let entry = self.modules.get_mut(&id).unwrap();
                entry.stamp = index as u64 + 1;
                if entry.pins == 0 && !entry.retired {
                    self.lru.insert((entry.stamp, id));
                }
            }
            self.clock = self.modules.len() as u64;
        }
        self.clock += 1;
        self.clock
    }

    fn touch(&mut self, id: ModuleId) {
        let stamp = self.stamp();
        let entry = self.modules.get_mut(&id).unwrap();
        self.lru.remove(&(entry.stamp, id));
        entry.stamp = stamp;
        if entry.pins == 0 && !entry.retired {
            self.lru.insert((stamp, id));
        }
    }

    fn unlink(&mut self, id: ModuleId) {
        for pc in self.modules[&id].code.entries() {
            if self.by_pc.get(pc) == Some(&id) {
                self.by_pc.remove(pc);
            }
        }
    }

    fn remove(&mut self, id: ModuleId) {
        self.unlink(id);
        let entry = self.modules.remove(&id).unwrap();
        assert_eq!(entry.pins, 0);
        self.lru.remove(&(entry.stamp, id));
        self.charged -= entry.charge;
        if !entry.retired {
            self.retired(id);
        }
        self.changed();
    }

    fn retire(&mut self, id: ModuleId) {
        let Some(entry) = self.modules.get(&id) else {
            return;
        };
        if entry.pins == 0 {
            self.remove(id);
        } else if !entry.retired {
            self.unlink(id);
            self.modules.get_mut(&id).unwrap().retired = true;
            self.retired(id);
            self.changed();
        }
    }

    pub fn live(&mut self, memory: &Memory, id: ModuleId) -> bool {
        let valid = self
            .modules
            .get(&id)
            .is_some_and(|entry| !entry.retired && entry.code.valid(memory));
        if !valid {
            self.retire(id);
        }
        valid
    }

    pub fn ready(&mut self, memory: &Memory, pc: u64, id: ModuleId) -> bool {
        self.live(memory, id) && self.modules[&id].code.can_enter(pc)
    }

    pub fn find(&mut self, memory: &Memory, pc: u64) -> Option<ModuleId> {
        let id = *self.by_pc.get(&pc)?;
        if !self.live(memory, id) {
            return None;
        }
        self.touch(id);
        Some(id)
    }

    pub(crate) fn covers(&mut self, memory: &Memory, pc: u64) -> bool {
        let Some(&id) = self.by_pc.get(&pc) else {
            return false;
        };
        self.live(memory, id)
    }

    pub fn insert(&mut self, code: Compiled) -> Result<ModuleId, CacheError> {
        let charge = code.storage_charge();
        if charge > self.limit || self.pinned_bytes > self.limit - charge {
            return Err(CacheError::Capacity);
        }
        let id = NEXT_ID
            .with(|next| {
                let id = next.get();
                next.set(id.checked_add(1)?);
                Some(id)
            })
            .ok_or(CacheError::IdentityExhausted)?;
        while self.charged > self.limit - charge {
            let &(_, oldest) = self.lru.first().expect("evictable module budget");
            self.remove(oldest);
        }
        let stamp = self.stamp();
        for &pc in code.entries() {
            self.by_pc.insert(pc, id);
        }
        self.modules.insert(
            id,
            Entry {
                code,
                charge,
                stamp,
                pins: 0,
                retired: false,
                slot: 0,
            },
        );
        self.charged += charge;
        self.lru.insert((stamp, id));
        Ok(id)
    }

    /// Retains module storage through a balanced release, including revocation.
    pub fn pin(&mut self, memory: &Memory, id: ModuleId) -> bool {
        if !self.live(memory, id) {
            return false;
        }
        let Some(pins) = self.modules[&id].pins.checked_add(1) else {
            return false;
        };
        let Some(total) = self.pins.checked_add(1) else {
            return false;
        };
        self.touch(id);
        let entry = self.modules.get_mut(&id).unwrap();
        if entry.pins == 0 {
            self.pinned_bytes += entry.charge;
            self.lru.remove(&(entry.stamp, id));
        }
        entry.pins = pins;
        self.pins = total;
        true
    }

    /// Missing or already unpinned identities are ignored.
    pub fn release(&mut self, memory: &Memory, id: ModuleId) {
        let Some(entry) = self.modules.get_mut(&id).filter(|entry| entry.pins != 0) else {
            return;
        };
        entry.pins -= 1;
        self.pins -= 1;
        if entry.pins == 0 {
            self.pinned_bytes -= entry.charge;
        }
        if entry.retired || !entry.code.valid(memory) {
            self.retire(id);
        } else {
            self.touch(id);
        }
    }

    /// A budget smaller than pinned storage is rejected without changing it.
    pub fn set_limit(&mut self, limit: usize) -> Result<(), CacheError> {
        if self.pinned_bytes > limit {
            return Err(CacheError::Capacity);
        }
        self.limit = limit;
        while self.charged > limit {
            let &(_, oldest) = self.lru.first().expect("evictable module budget");
            self.remove(oldest);
        }
        Ok(())
    }

    pub fn prune_invalid(&mut self, memory: &Memory) {
        let invalid: Vec<_> = self
            .modules
            .iter()
            .filter(|(_, entry)| !entry.code.valid(memory))
            .map(|(&id, _)| id)
            .collect();
        for id in invalid {
            self.retire(id);
        }
    }

    /// Retires all identities; pinned storage remains until its final release.
    pub fn clear(&mut self) {
        for id in self.modules.keys().copied().collect::<Vec<_>>() {
            self.retire(id);
        }
    }
}
