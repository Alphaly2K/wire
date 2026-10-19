//! Query-only identity of wire's ordinary, confined guest user. No host credentials or elevation.
use super::{Memory, WRITE};
use std::collections::BTreeMap;

struct Token {
    access: u32,
    flags: u32,
}

pub(super) struct Tokens {
    handles: BTreeMap<u64, Token>,
    next: u64,
}
impl Default for Tokens {
    fn default() -> Self {
        Self {
            handles: BTreeMap::new(),
            next: 0x7fff4000,
        }
    }
}

fn error(mem: &mut Memory, last_error: u64, code: u32) -> Result<u64, String> {
    mem.write_int(last_error, 4, code as u64)?;
    Ok(0)
}

impl Tokens {
    pub(super) fn open(
        &mut self,
        mem: &mut Memory,
        args: &[u64],
        last_error: u64,
    ) -> Result<u64, String> {
        if args[0] != u64::MAX {
            return error(mem, last_error, 6);
        }
        if args[1] & !8 != 0 {
            return error(mem, last_error, 50);
        }
        if self.handles.len() >= 128 || self.next >= 0x7fff5000 {
            return error(mem, last_error, 8);
        }
        // This API's capacity check precedes output preflight. Do not use the
        // opposite order required by EventRegister's independent contract.
        mem.check(args[2], 8, WRITE)?;
        let handle = self.next;
        mem.write_int(args[2], 8, handle)?;
        self.next += 1;
        self.handles.insert(
            handle,
            Token {
                access: args[1] as u32,
                flags: 0,
            },
        );
        Ok(1)
    }

    pub(super) fn query(
        &self,
        mem: &mut Memory,
        args: &[u64],
        last_error: u64,
    ) -> Result<u64, String> {
        if !self.handles.contains_key(&args[0]) {
            return error(mem, last_error, 6);
        }
        if self.handles[&args[0]].access & 8 == 0 {
            return error(mem, last_error, 5);
        }
        let data = match information(args[1] as u32, args[2]) {
            Ok(data) => data,
            Err(code) => return error(mem, last_error, code),
        };
        mem.check(args[4], 4, WRITE)?;
        if args[3] < data.len() as u64 {
            mem.write_int(args[4], 4, data.len() as u64)?;
            return error(mem, last_error, 122);
        }
        mem.check(args[2], data.len(), WRITE)?;
        mem.write(args[2], &data)?;
        mem.write_int(args[4], 4, data.len() as u64)?;
        Ok(1)
    }

    pub(super) fn flags(&self, handle: u64) -> Option<u32> {
        self.handles.get(&handle).map(|token| token.flags)
    }

    pub(super) fn set_flags(
        &mut self,
        handle: u64,
        mask: u64,
        value: u64,
    ) -> Option<Result<(), u32>> {
        let token = self.handles.get_mut(&handle)?;
        if mask & !3 != 0 {
            return Some(Err(87));
        }
        token.flags = token.flags & !(mask as u32) | value as u32 & mask as u32;
        Some(Ok(()))
    }

    pub(super) fn close(&mut self, handle: u64) -> Result<(), u32> {
        if self.flags(handle).ok_or(6u32)? & 2 != 0 {
            return Err(6);
        }
        self.handles.remove(&handle);
        Ok(())
    }
}

fn information(class: u32, output: u64) -> Result<Vec<u8>, u32> {
    let data = match class {
        18 => 1u32.to_le_bytes().to_vec(), // TokenElevationTypeDefault: no linked elevated token.
        20 | 29 => 0u32.to_le_bytes().to_vec(),
        8 => 1u32.to_le_bytes().to_vec(), // TokenPrimary
        1 => {
            let mut bytes = vec![0; 44];
            bytes[..8].copy_from_slice(&output.checked_add(16).ok_or(87u32)?.to_le_bytes());
            bytes[16..24].copy_from_slice(&[1, 5, 0, 0, 0, 0, 0, 5]);
            for (n, part) in [21u32, 1, 1, 1, 1000].iter().enumerate() {
                bytes[24 + n * 4..28 + n * 4].copy_from_slice(&part.to_le_bytes());
            }
            bytes
        }
        25 => {
            let mut bytes = vec![0; 28];
            bytes[..8].copy_from_slice(&output.checked_add(16).ok_or(87u32)?.to_le_bytes());
            bytes[8..12].copy_from_slice(&0x60u32.to_le_bytes());
            bytes[16..24].copy_from_slice(&[1, 1, 0, 0, 0, 0, 0, 16]);
            bytes[24..28].copy_from_slice(&8192u32.to_le_bytes());
            bytes
        }
        _ => return Err(50),
    };
    // Keep structure padding zero; guest pointers are filled only for pointer-bearing classes.
    Ok(data)
}
