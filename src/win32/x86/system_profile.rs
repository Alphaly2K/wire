//! PE32 hardware layout and memory accounting; no process-wide state borrow.
use crate::{mem::GuestMem, win32::catalog::SystemProfileApi};

const PHYSICAL: u64 = 512 * 1024 * 1024;
const VIRTUAL: u64 = 0x7fff0000;

pub(crate) fn call(
    id: SystemProfileApi,
    pointer: u32,
    mem: &GuestMem,
    last_error: &mut u32,
) -> (u32, String) {
    let result = (|| -> Result<u32, u32> {
        match id {
            SystemProfileApi::SystemInfo => {
                let mut out = [0u8; 36];
                for (offset, value) in [
                    (0, 0u32),
                    (4, 4096),
                    (8, 0x10000),
                    (12, 0x7ffeffff),
                    (16, 1),
                    (20, 1),
                    (24, 586),
                    (28, 65536),
                ] {
                    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                }
                out[32..34].copy_from_slice(&6u16.to_le_bytes());
                out[34..36].copy_from_slice(&0x3a09u16.to_le_bytes());
                mem.write_bytes(pointer, &out).map_err(|_| 87u32)?;
                Ok(0)
            }
            SystemProfileApi::MemoryStatus | SystemProfileApi::MemoryStatusEx => {
                let used = mem.application_committed_bytes();
                let available = PHYSICAL.saturating_sub(used);
                let load = ((PHYSICAL - available) * 100 / PHYSICAL) as u32;
                let values = [
                    PHYSICAL,
                    available,
                    PHYSICAL * 2,
                    PHYSICAL * 2 - used.min(PHYSICAL * 2),
                    VIRTUAL,
                    VIRTUAL.saturating_sub(mem.application_reserved_bytes()),
                    0,
                ];
                if id == SystemProfileApi::MemoryStatusEx {
                    if mem.read_u32(pointer).map_err(|_| 87u32)? != 64 {
                        return Err(87);
                    }
                    let mut out = [0u8; 64];
                    out[..4].copy_from_slice(&64u32.to_le_bytes());
                    out[4..8].copy_from_slice(&load.to_le_bytes());
                    for (i, value) in values.iter().enumerate() {
                        out[8 + i * 8..16 + i * 8].copy_from_slice(&value.to_le_bytes());
                    }
                    mem.write_bytes(pointer, &out).map_err(|_| 87u32)?;
                    Ok(1)
                } else {
                    let mut out = [0u8; 32];
                    out[..4].copy_from_slice(&32u32.to_le_bytes());
                    out[4..8].copy_from_slice(&load.to_le_bytes());
                    for (i, value) in values[..6].iter().enumerate() {
                        out[8 + i * 4..12 + i * 4].copy_from_slice(&(*value as u32).to_le_bytes());
                    }
                    mem.write_bytes(pointer, &out).map_err(|_| 87u32)?;
                    Ok(0)
                }
            }
        }
    })();
    match result {
        Ok(value) => (value, "x86 guest hardware / monotonic clock profile".into()),
        Err(error) => {
            *last_error = error;
            (0, format!("Win32 error {error}"))
        }
    }
}
