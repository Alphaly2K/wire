//! Owned sparse pages with a small address-to-slot cache. Cached slots contain
//! no pointer or permission proof; callers inspect the live Page on every use.
use super::Page;
use std::{
    cell::Cell,
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
    ops::Index,
};

const CACHE_SLOTS: usize = 64;

// Aligned page numbers are integer keys. The full guest address is still
// compared by HashMap and by the cache, including on hash collisions.
#[derive(Default)]
struct PageHasher(u64);
impl Hasher for PageHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write_u64(&mut self, address: u64) {
        let mixed = (address >> 12).wrapping_mul(0x9e3779b97f4a7c15);
        self.0 = mixed ^ (mixed >> 32);
    }
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = self.0.wrapping_mul(0x100000001b3) ^ byte as u64;
        }
    }
}

pub(super) struct Pages {
    indices: HashMap<u64, usize, BuildHasherDefault<PageHasher>>,
    slots: Vec<Option<(u64, Page)>>,
    free: Vec<usize>,
    recent: [Cell<Option<(u64, usize)>>; CACHE_SLOTS],
}

impl Default for Pages {
    fn default() -> Self {
        Self {
            indices: HashMap::default(),
            slots: Vec::new(),
            free: Vec::new(),
            recent: std::array::from_fn(|_| Cell::new(None)),
        }
    }
}

impl Pages {
    fn cache_slot(address: u64) -> usize {
        let page = address >> 12;
        ((page ^ (page >> 16) ^ (page >> 32)) as usize) & (CACHE_SLOTS - 1)
    }

    fn slot(&self, address: &u64) -> Option<usize> {
        let recent = &self.recent[Self::cache_slot(*address)];
        if let Some((at, index)) = recent.get() {
            if at == *address {
                return Some(index);
            }
        }
        let index = *self.indices.get(address)?;
        recent.set(Some((*address, index)));
        Some(index)
    }

    pub(super) fn get(&self, address: &u64) -> Option<&Page> {
        self.slots[self.slot(address)?]
            .as_ref()
            .map(|(_, page)| page)
    }

    pub(super) fn get_mut(&mut self, address: &u64) -> Option<&mut Page> {
        let index = self.slot(address)?;
        self.slots[index].as_mut().map(|(_, page)| page)
    }

    pub(super) fn contains_key(&self, address: &u64) -> bool {
        self.slot(address).is_some()
    }

    pub(super) fn insert(&mut self, address: u64, page: Page) {
        let index = if let Some(index) = self.slot(&address) {
            index
        } else if let Some(index) = self.free.pop() {
            index
        } else {
            self.slots.push(None);
            self.slots.len() - 1
        };
        self.slots[index] = Some((address, page));
        self.indices.insert(address, index);
        self.recent[Self::cache_slot(address)].set(Some((address, index)));
    }

    pub(super) fn remove(&mut self, address: &u64) -> Option<Page> {
        let index = self.indices.remove(address)?;
        // Clear the address cache before the slot can be reused or moved into
        // a different Memory. Removal never leaves a reference to freed data.
        let recent = &self.recent[Self::cache_slot(*address)];
        if recent.get().is_some_and(|(at, _)| at == *address) {
            recent.set(None);
        }
        self.free.push(index);
        self.slots[index].take().map(|(_, page)| page)
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(&u64, &mut Page) -> bool) {
        for index in 0..self.slots.len() {
            let remove = self.slots[index]
                .as_mut()
                .and_then(|(address, page)| (!keep(address, page)).then_some(*address));
            if let Some(address) = remove {
                self.remove(&address);
            }
        }
    }

    pub(super) fn len(&self) -> usize {
        self.indices.len()
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &Page> {
        self.slots.iter().flatten().map(|(_, page)| page)
    }

    pub(super) fn into_entries(self) -> impl Iterator<Item = (u64, Page)> {
        self.slots.into_iter().flatten()
    }
}

impl Index<&u64> for Pages {
    type Output = Page;

    fn index(&self, address: &u64) -> &Self::Output {
        self.get(address).expect("checked owned AMD64 page")
    }
}
