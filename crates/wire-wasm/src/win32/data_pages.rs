//! Runtime data blocks keep ownership separate from public guest heaps.
use super::ServiceError;
use crate::memory::{Memory, PAGE};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Pages {
    blocks: BTreeMap<u64, u64>,
}
impl Pages {
    pub(super) fn adopt(&mut self, base: u64, span: u64) {
        if let Some(old) = self.blocks.insert(base, span) {
            assert_eq!(old, span);
        }
    }
    pub(super) fn span(&self, address: u64) -> Option<u64> {
        self.blocks.get(&address).copied()
    }
    pub(super) fn check_free(&self, memory: &Memory, address: u64) -> Result<(), ServiceError> {
        let span = self
            .span(address)
            .ok_or(ServiceError::Unavailable("invalid runtime data release"))?;
        memory
            .check_unmap(address, span)
            .map_err(|_| ServiceError::Unavailable("runtime data release failed"))
    }
    pub(super) fn allocate(&mut self, memory: &mut Memory, size: u64) -> u64 {
        if size > 16 * 1024 * 1024 {
            return 0;
        }
        let span = size.max(1).div_ceil(PAGE) * PAGE;
        let Ok(address) = memory.map_heap(span, false) else {
            return 0;
        };
        self.blocks.insert(address, span);
        address
    }
    pub(super) fn readonly(&self, memory: &mut Memory, address: u64) -> Result<(), ServiceError> {
        if !self.blocks.contains_key(&address) {
            return Err(ServiceError::Unavailable("invalid runtime data allocation"));
        }
        memory
            .seal_runtime_readonly(address)
            .map_err(|_| ServiceError::Unavailable("runtime data protection failed"))
    }
    pub(crate) fn free(&mut self, memory: &mut Memory, address: u64) -> Result<(), ServiceError> {
        let span = *self
            .blocks
            .get(&address)
            .ok_or(ServiceError::Unavailable("invalid runtime data release"))?;
        memory
            .unmap(address, span)
            .map_err(|_| ServiceError::Unavailable("runtime data release failed"))?;
        self.blocks.remove(&address);
        Ok(())
    }
    pub(crate) fn check_clear(&self, memory: &Memory) -> Result<(), String> {
        for (&address, &span) in &self.blocks {
            memory.check_unmap(address, span)?;
        }
        Ok(())
    }
    pub(crate) fn clear(&mut self, memory: &mut Memory) {
        for (address, span) in std::mem::take(&mut self.blocks) {
            memory
                .unmap(address, span)
                .expect("preflighted runtime data cleanup");
        }
    }
}
