//! Win64 VMM adapter; checked memory and invalidation stay owned by Memory.
use crate::win32::catalog::VirtualMemoryApi;
use crate::x64::memory::{Memory, WRITE};

pub(crate) fn call(
    mem: &mut Memory,
    id: VirtualMemoryApi,
    args: &[u64],
    last_error: u64,
) -> Result<u64, String> {
    let offset = id.extended() as usize;
    let result: Result<u64, u32> = if id.extended() && args[0] != u64::MAX {
        Err(6)
    } else {
        let a = &args[offset..];
        match id.base() {
            VirtualMemoryApi::Alloc => mem.virtual_alloc(a[0], a[1], a[2] as u32, a[3] as u32),
            VirtualMemoryApi::Free => mem.virtual_free(a[0], a[1], a[2] as u32).map(|()| 1),
            VirtualMemoryApi::Protect => {
                mem.check(a[3], 4, WRITE)?;
                match mem.validate_protect(a[0], a[1], a[2] as u32) {
                    Ok(old) => {
                        // Copy out before applying permissions, including output/target alias.
                        mem.write_int(a[3], 4, old as u64)?;
                        mem.virtual_protect(a[0], a[1], a[2] as u32).map(|_| 1)
                    }
                    Err(error) => Err(error),
                }
            }
            VirtualMemoryApi::Query => {
                if a[2] < 48 {
                    Err(24)
                } else {
                    match mem.virtual_query(a[0]) {
                        Ok(info) => {
                            mem.write(a[1], &info)?;
                            Ok(48)
                        }
                        Err(error) => Err(error),
                    }
                }
            }
            _ => unreachable!(),
        }
    };
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            mem.write_int(last_error, 4, error as u64)?;
            Ok(0)
        }
    }
}
