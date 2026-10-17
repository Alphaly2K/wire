//! Win64 BSTR membership, backing identity and reallocation validation.
use crate::x64::memory::Memory;
use std::collections::BTreeMap;

#[derive(Default)]
pub(in crate::x64::runtime) struct Bstrings {
    blocks: BTreeMap<u64, (u64, usize)>,
}

impl Bstrings {
    pub(super) fn track(&mut self, pointer: u64, base: u64, length: usize) {
        self.blocks.insert(pointer, (base, length));
    }

    pub(super) fn length(&self, pointer: u64) -> usize {
        self.blocks[&pointer].1
    }

    pub(super) fn validate_replacement(
        &self,
        allocations: &BTreeMap<u64, (usize, u64)>,
        old: u64,
        output: u64,
    ) -> Result<(), String> {
        if old != 0 && !self.blocks.contains_key(&old) {
            return Err("invalid BSTR reallocation source".into());
        }
        if old != 0 {
            let base = self.blocks[&old].0;
            let span = allocations[&base].1;
            if output < base + span && output + 8 > base {
                return Err("BSTR output overlaps allocation being replaced".into());
            }
        }
        Ok(())
    }

    pub(super) fn free(
        &mut self,
        mem: &mut Memory,
        allocations: &mut BTreeMap<u64, (usize, u64)>,
        pointer: u64,
    ) -> Result<(), String> {
        if pointer == 0 {
            return Ok(());
        }
        let &(base, _) = self
            .blocks
            .get(&pointer)
            .ok_or("invalid or released BSTR allocation")?;
        mem.unmap(base, allocations[&base].1)?;
        allocations.remove(&base);
        self.blocks.remove(&pointer);
        Ok(())
    }
}
