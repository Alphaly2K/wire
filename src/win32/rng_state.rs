//! RNG provider identities and validation, independent of entropy and memory.
use std::collections::BTreeSet;

pub(crate) const LIMIT: u32 = 16 * 1024 * 1024;
pub(crate) struct Providers {
    handles: BTreeSet<u64>,
    start: u64,
}
impl Providers {
    pub(crate) fn new(start: u64) -> Self {
        Self {
            handles: BTreeSet::new(),
            start,
        }
    }
    pub(crate) fn validate(&self, handle: u64, flags: u32, length: u32) -> Result<(), u64> {
        if flags & !3 != 0 || flags & 2 != 0 && handle != 0 {
            return Err(0xc000000d);
        }
        if flags & 2 == 0 && handle != 0x81 && !self.handles.contains(&handle) {
            return Err(0xc0000008);
        }
        if length > LIMIT {
            return Err(0xc0000017);
        }
        Ok(())
    }
    pub(crate) fn vacant(&self) -> Option<u64> {
        (0..1024)
            .map(|n| self.start + n * 16)
            .find(|h| !self.handles.contains(h))
    }
    pub(crate) fn insert(&mut self, handle: u64) {
        assert!(self.handles.insert(handle));
    }
    pub(crate) fn close(&mut self, handle: u64) -> bool {
        self.handles.remove(&handle)
    }
    #[allow(dead_code)]
    pub(crate) fn clear(&mut self) {
        self.handles.clear();
    }
}
