//! One cooperative API call is indivisible with respect to guest scheduling.
//! Memory checks and the caller's return-frame submission retain their order.
use crate::{api::Args, mem::GuestMem, win32::catalog::InterlockedApi};

pub(crate) fn call(id: InterlockedApi, a: &Args, mem: &mut GuestMem) -> Result<u32, String> {
    use InterlockedApi::*;
    match id {
        InitializeSListHead | FlushSList => {
            if a[0] & 7 != 0 {
                return Err("unaligned SLIST_HEADER".into());
            }
            let previous = mem.read_u64(a[0]).map_err(|e| e.to_string())?;
            if id == InitializeSListHead {
                mem.write_u64(a[0], 0).map_err(|e| e.to_string())?;
                Ok(0)
            } else {
                let sequence = ((previous >> 48) as u16).wrapping_add(1);
                mem.write_u64(a[0], (sequence as u64) << 48)
                    .map_err(|e| e.to_string())?;
                Ok(previous as u32)
            }
        }
        Increment | Decrement => {
            let old = mem.read_u32(a[0]).map_err(|e| e.to_string())?;
            let value = if id == Increment {
                old.wrapping_add(1)
            } else {
                old.wrapping_sub(1)
            };
            mem.write_u32(a[0], value).map_err(|e| e.to_string())?;
            Ok(value)
        }
        Exchange | CompareExchange => {
            let old = mem.read_u32(a[0]).map_err(|e| e.to_string())?;
            if id == Exchange || old == a[2] {
                mem.write_u32(a[0], a[1]).map_err(|e| e.to_string())?;
            }
            Ok(old)
        }
    }
}
