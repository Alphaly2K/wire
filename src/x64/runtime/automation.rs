//! Independent BSTR data layout and allocator ownership. No host pointers.
use super::{allocation::Pages, Memory, WRITE};
use crate::win32::catalog::BstrApi;
mod strings;
pub(super) use strings::Bstrings;

fn bstr_source(mem: &Memory, p: u64, length: Option<u32>) -> Result<Option<Vec<u8>>, String> {
    if let Some(length) = length {
        if length > 16 * 1024 * 1024 - 6 {
            return Ok(None);
        }
        let mut bytes = vec![0; length as usize];
        if p != 0 {
            mem.read(p, &mut bytes)?;
        }
        return Ok(Some(bytes));
    }
    if p == 0 {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    for i in 0..8 * 1024 * 1024 - 3 {
        let at = p.checked_add(i * 2).ok_or("BSTR source pointer overflow")?;
        let unit = mem.read_int(at, 2)? as u16;
        if unit == 0 {
            return Ok(Some(bytes));
        }
        bytes.extend(unit.to_le_bytes());
    }
    Ok(None)
}
impl Bstrings {
    fn allocate(
        &mut self,
        mem: &mut Memory,
        bytes: &[u8],
        pages: &mut Pages<'_>,
    ) -> Result<u64, String> {
        let base = pages.allocate(mem, bytes.len() as u64 + 6)?;
        if base == 0 {
            return Ok(0);
        }
        mem.write_int(base, 4, bytes.len() as u64)?;
        mem.write(base + 4, bytes)?;
        mem.write(base + 4 + bytes.len() as u64, &[0; 2])?;
        self.track(base + 4, base, bytes.len());
        Ok(base + 4)
    }
    pub(super) fn call(
        &mut self,
        mem: &mut Memory,
        id: BstrApi,
        a: &[u64],
        pages: &mut Pages<'_>,
    ) -> Result<u64, String> {
        use BstrApi::*;
        match id {
            Length | ByteLength => {
                if a[0] == 0 {
                    return Ok(0);
                }
                let length =
                    mem.read_int(a[0].checked_sub(4).ok_or("BSTR prefix underflow")?, 4)?;
                Ok(if id == Length { length / 2 } else { length })
            }
            Free => {
                self.free(mem, pages.allocations, a[0])?;
                Ok(0)
            }
            Allocate | AllocateLength | AllocateBytes => {
                let length = if id == Allocate {
                    None
                } else if id == AllocateBytes {
                    Some(a[1] as u32)
                } else {
                    let Some(n) = (a[1] as u32).checked_mul(2) else {
                        return Ok(0);
                    };
                    Some(n)
                };
                let Some(bytes) = bstr_source(mem, a[0], length)? else {
                    return Ok(0);
                };
                self.allocate(mem, &bytes, pages)
            }
            Reallocate | ReallocateLength => {
                if a[0] == 0 {
                    return Ok(0);
                }
                mem.check(a[0], 8, WRITE)?;
                let old = mem.read_int(a[0], 8)?;
                self.validate_replacement(pages.allocations, old, a[0])?;
                if id == Reallocate && a[1] == 0 {
                    self.free(mem, pages.allocations, old)?;
                    mem.write_int(a[0], 8, 0)?;
                    return Ok(1);
                }
                let length = if id == ReallocateLength {
                    let Some(n) = (a[2] as u32).checked_mul(2) else {
                        return Ok(0);
                    };
                    Some(n)
                } else {
                    None
                };
                let Some(mut bytes) = bstr_source(mem, a[1], length)? else {
                    return Ok(0);
                };
                if id == ReallocateLength && a[1] == 0 && old != 0 {
                    let keep = self.length(old).min(bytes.len());
                    mem.read(old, &mut bytes[..keep])?;
                }
                // Read aliasing source before allocating/freeing the old string.
                let next = self.allocate(mem, &bytes, pages)?;
                if next == 0 {
                    return Ok(0);
                }
                self.free(mem, pages.allocations, old)?;
                mem.write_int(a[0], 8, next)?;
                Ok(1)
            }
        }
    }
}
