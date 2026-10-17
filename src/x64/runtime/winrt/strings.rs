//! Own HSTRING membership, reference counts and the cached empty buffer.
//! Reference headers and backing allocations remain guest memory resources.
use crate::x64::memory::{Memory, READ};
use std::collections::BTreeMap;

#[derive(Default)]
pub(in crate::x64::runtime) struct Strings {
    owned: BTreeMap<u64, (u32, u32)>, // length, references
    empty: u64,
}

pub(super) enum Retention {
    Reference,
    Retained,
    Overflow,
}

impl Strings {
    pub(super) fn parts(&self, mem: &Memory, handle: u64) -> Result<(u64, u32), String> {
        if handle == 0 {
            return Ok((0, 0));
        }
        if let Some(&(length, _)) = self.owned.get(&handle) {
            mem.check(handle, 24, READ)?;
            return Ok((handle + 32, length));
        }
        // Caller storage can change between calls; never cache reference metadata.
        let mut header = [0; 24];
        mem.read(handle, &mut header)?;
        let flags = u32::from_le_bytes(header[..4].try_into().unwrap());
        let length = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let pointer = u64::from_le_bytes(header[16..24].try_into().unwrap());
        if flags != 1 || length > super::MAX_UNITS || pointer == 0 {
            return Err("invalid Win64 HSTRING reference header".into());
        }
        mem.check(pointer, (length as usize + 1) * 2, READ)?;
        Ok((pointer, length))
    }

    pub(super) fn bytes(&self, mem: &Memory, handle: u64) -> Result<Vec<u8>, String> {
        let (pointer, length) = self.parts(mem, handle)?;
        let mut data = vec![0; length as usize * 2];
        mem.read(pointer, &mut data)?;
        Ok(data)
    }

    pub(super) fn can_copy(&self, length: usize) -> bool {
        length <= super::MAX_UNITS as usize && self.owned.len() < 4096
    }

    pub(super) fn track(&mut self, handle: u64, length: u32) {
        self.owned.insert(handle, (length, 1));
    }

    pub(super) fn owns(&self, handle: u64) -> bool {
        self.owned.contains_key(&handle)
    }

    pub(super) fn release(
        &mut self,
        mem: &mut Memory,
        allocations: &mut BTreeMap<u64, (usize, u64)>,
        handle: u64,
    ) -> Result<(), String> {
        if let Some(&(_, refs)) = self.owned.get(&handle) {
            if refs == 1 {
                mem.unmap(handle, allocations[&handle].1)?;
                allocations.remove(&handle);
                self.owned.remove(&handle);
            } else {
                self.owned.get_mut(&handle).unwrap().1 -= 1;
            }
        } else {
            self.parts(mem, handle)?;
        }
        Ok(())
    }

    pub(super) fn retain(&mut self, handle: u64) -> Retention {
        let Some((_, refs)) = self.owned.get_mut(&handle) else {
            return Retention::Reference;
        };
        let Some(next) = refs.checked_add(1) else {
            return Retention::Overflow;
        };
        *refs = next;
        Retention::Retained
    }

    pub(super) fn empty_buffer(&self) -> u64 {
        self.empty
    }

    pub(super) fn cache_empty_buffer(&mut self, pointer: u64) {
        self.empty = pointer;
    }
}
