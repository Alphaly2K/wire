//! Independent BSTR data layout and allocator ownership. No host pointers.
use super::*;
pub(super) fn symbol<'a>(dll: &str, name: &'a str) -> &'a str {
    if !dll.eq_ignore_ascii_case("oleaut32.dll") {
        return name;
    }
    match name {
        "#2" => "SysAllocString",
        "#3" => "SysReAllocString",
        "#4" => "SysAllocStringLen",
        "#5" => "SysReAllocStringLen",
        "#6" => "SysFreeString",
        "#7" => "SysStringLen",
        "#149" => "SysStringByteLen",
        "#150" => "SysAllocStringByteLen",
        _ => name,
    }
}
pub(super) fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "SysAllocString" | "SysFreeString" | "SysStringLen" | "SysStringByteLen" => 1,
        "SysAllocStringLen" | "SysReAllocString" | "SysAllocStringByteLen" => 2,
        "SysReAllocStringLen" => 3,
        _ => return None,
    })
}
pub(super) fn export(dll: &str, name: &str) -> bool {
    dll.eq_ignore_ascii_case("oleaut32.dll") && arity(name).is_some()
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    match name {
        "SysAllocStringLen" | "SysAllocStringByteLen" => &[1],
        "SysReAllocStringLen" => &[2],
        _ => &[],
    }
}
impl Runtime64 {
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
    fn bstr_allocate(&mut self, mem: &mut Memory, bytes: &[u8]) -> Result<u64, String> {
        let base = self.heap_alloc(mem, bytes.len() as u64 + 6)?;
        if base == 0 {
            return Ok(0);
        }
        mem.write_int(base, 4, bytes.len() as u64)?;
        mem.write(base + 4, bytes)?;
        mem.write(base + 4 + bytes.len() as u64, &[0; 2])?;
        self.bstrings.insert(base + 4, (base, bytes.len()));
        Ok(base + 4)
    }
    fn bstr_free(&mut self, mem: &mut Memory, p: u64) -> Result<(), String> {
        if p == 0 {
            return Ok(());
        }
        let &(base, _) = self
            .bstrings
            .get(&p)
            .ok_or("invalid or released BSTR allocation")?;
        let span = self.allocations[&base].1;
        mem.unmap(base, span)?;
        self.allocations.remove(&base);
        self.bstrings.remove(&p);
        Ok(())
    }
    pub(super) fn automation_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        match name {
            "SysStringLen" | "SysStringByteLen" => {
                if a[0] == 0 {
                    return Ok(0);
                }
                let length =
                    mem.read_int(a[0].checked_sub(4).ok_or("BSTR prefix underflow")?, 4)?;
                Ok(if name == "SysStringLen" {
                    length / 2
                } else {
                    length
                })
            }
            "SysFreeString" => {
                self.bstr_free(mem, a[0])?;
                Ok(0)
            }
            "SysAllocString" | "SysAllocStringLen" | "SysAllocStringByteLen" => {
                let length = if name == "SysAllocString" {
                    None
                } else if name.ends_with("ByteLen") {
                    Some(a[1] as u32)
                } else {
                    let Some(n) = (a[1] as u32).checked_mul(2) else {
                        return Ok(0);
                    };
                    Some(n)
                };
                let Some(bytes) = Self::bstr_source(mem, a[0], length)? else {
                    return Ok(0);
                };
                self.bstr_allocate(mem, &bytes)
            }
            "SysReAllocString" | "SysReAllocStringLen" => {
                if a[0] == 0 {
                    return Ok(0);
                }
                mem.check(a[0], 8, WRITE)?;
                let old = mem.read_int(a[0], 8)?;
                if old != 0 && !self.bstrings.contains_key(&old) {
                    return Err("invalid BSTR reallocation source".into());
                }
                if old != 0 {
                    let base = self.bstrings[&old].0;
                    let span = self.allocations[&base].1;
                    if a[0] < base + span && a[0] + 8 > base {
                        return Err("BSTR output overlaps allocation being replaced".into());
                    }
                }
                if name == "SysReAllocString" && a[1] == 0 {
                    self.bstr_free(mem, old)?;
                    mem.write_int(a[0], 8, 0)?;
                    return Ok(1);
                }
                let length = if name.ends_with("Len") {
                    let Some(n) = (a[2] as u32).checked_mul(2) else {
                        return Ok(0);
                    };
                    Some(n)
                } else {
                    None
                };
                let Some(mut bytes) = Self::bstr_source(mem, a[1], length)? else {
                    return Ok(0);
                };
                if name.ends_with("Len") && a[1] == 0 && old != 0 {
                    let keep = self.bstrings[&old].1.min(bytes.len());
                    mem.read(old, &mut bytes[..keep])?;
                }
                // Read aliasing source before allocating/freeing the old string.
                let next = self.bstr_allocate(mem, &bytes)?;
                if next == 0 {
                    return Ok(0);
                }
                self.bstr_free(mem, old)?;
                mem.write_int(a[0], 8, next)?;
                Ok(1)
            }
            _ => Err(format!("unsupported automation call {name}")),
        }
    }
}
