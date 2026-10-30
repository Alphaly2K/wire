//! Fixed-size PC links for one resident execution. A cached function slot is
//! authorized by its run lease, source/mapping authority, and cache epoch.
use std::cell::Cell;

pub(crate) const LINK_SLOTS: usize = 64;

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct Link {
    pub pc: u64,
    pub run: u64,
    pub code_epoch: u64,
    pub mapping_epoch: u64,
    pub source_epoch: u64,
    pub slot: u32,
}

#[repr(C)]
pub struct LinkCache {
    pub(crate) epoch: Cell<u64>,
    pub(crate) run: Cell<u64>,
    pub(crate) entries: [Cell<Link>; LINK_SLOTS],
}

impl Default for LinkCache {
    fn default() -> Self {
        Self {
            epoch: Cell::new(1),
            run: Cell::new(0),
            entries: std::array::from_fn(|_| Cell::new(Link::default())),
        }
    }
}

impl LinkCache {
    pub(crate) fn record(&self, pc: u64, mapping_epoch: u64, source_epoch: u64, slot: u32) {
        let index = ((pc ^ (pc >> 16) ^ (pc >> 32)) as usize) & (LINK_SLOTS - 1);
        self.entries[index].set(Link {
            pc,
            run: self.run.get(),
            code_epoch: self.epoch.get(),
            mapping_epoch,
            source_epoch,
            slot,
        });
    }
}
