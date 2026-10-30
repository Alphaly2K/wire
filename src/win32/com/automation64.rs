//! Shared Win64 BSTR allocation, membership and replacement semantics.
use crate::win32::catalog::{BstrApi, VariantApi};
use std::collections::BTreeMap;
pub(crate) trait Backend {
    type Error;
    fn check_write(&self, address: u64, size: usize) -> Result<(), Self::Error>;
    fn read(&self, address: u64, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Self::Error>;
    fn allocate(&mut self, size: u64) -> Result<u64, Self::Error>;
    fn span(&self, base: u64) -> Option<u64>;
    fn check_free(&self, base: u64) -> Result<(), Self::Error>;
    fn free(&mut self, base: u64) -> Result<(), Self::Error>;
    fn invalid(&self, message: &'static str) -> Self::Error;
    fn windows(code: u32) -> Self::Error;
    fn read_int(&self, address: u64, width: usize) -> Result<u64, Self::Error> {
        let mut bytes = [0; 8];
        self.read(address, &mut bytes[..width])?;
        Ok(u64::from_le_bytes(bytes))
    }
    fn write_int(&mut self, address: u64, width: usize, value: u64) -> Result<(), Self::Error> {
        self.write(address, &value.to_le_bytes()[..width])
    }
}
#[derive(Default)]
pub(crate) struct Bstrings {
    blocks: BTreeMap<u64, (u64, usize)>,
}
impl Bstrings {
    pub(crate) fn duplicate<B: Backend>(
        &mut self,
        mem: &mut B,
        source: Option<u64>,
    ) -> Result<u64, B::Error> {
        super::variant::Backend::string(
            &mut VariantAccess {
                strings: self,
                memory: mem,
            },
            source,
        )
    }
    pub(crate) fn check_string<B: Backend>(
        &self,
        mem: &B,
        p: u64,
        output: u64,
        size: usize,
    ) -> Result<(), B::Error> {
        if p != 0 && !self.blocks.contains_key(&p) {
            return Err(B::windows(super::variant::INVALID));
        }
        self.validate_replacement(mem, p, output, size)
    }
    pub(crate) fn free_string<B: Backend>(&mut self, mem: &mut B, p: u64) -> Result<(), B::Error> {
        self.free(mem, p)
    }
    fn track(&mut self, pointer: u64, base: u64, length: usize) {
        self.blocks.insert(pointer, (base, length));
    }
    fn length(&self, pointer: u64) -> usize {
        self.blocks[&pointer].1
    }
    fn validate_replacement<B: Backend>(
        &self,
        mem: &B,
        old: u64,
        output: u64,
        size: usize,
    ) -> Result<(), B::Error> {
        if old != 0 {
            let &(base, _) = self
                .blocks
                .get(&old)
                .ok_or_else(|| mem.invalid("invalid BSTR reallocation source"))?;
            let span = mem
                .span(base)
                .ok_or_else(|| mem.invalid("BSTR backing ownership was lost"))?;
            if output < base + span && output.saturating_add(size as u64) > base {
                return Err(mem.invalid("BSTR output overlaps allocation being replaced"));
            }
            mem.check_free(base)?;
        }
        Ok(())
    }
    fn free<B: Backend>(&mut self, mem: &mut B, pointer: u64) -> Result<(), B::Error> {
        if pointer == 0 {
            return Ok(());
        }
        let &(base, _) = self
            .blocks
            .get(&pointer)
            .ok_or_else(|| mem.invalid("invalid or released BSTR allocation"))?;
        mem.free(base)?;
        self.blocks.remove(&pointer);
        Ok(())
    }
}
fn bstr_source<B: Backend>(
    mem: &B,
    p: u64,
    length: Option<u32>,
) -> Result<Option<Vec<u8>>, B::Error> {
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
        let at = p
            .checked_add(i * 2)
            .ok_or_else(|| mem.invalid("BSTR source pointer overflow"))?;
        let unit = mem.read_int(at, 2)? as u16;
        if unit == 0 {
            return Ok(Some(bytes));
        }
        bytes.extend(unit.to_le_bytes());
    }
    Ok(None)
}
impl Bstrings {
    fn allocate<B: Backend>(&mut self, mem: &mut B, bytes: &[u8]) -> Result<u64, B::Error> {
        let base = mem.allocate(bytes.len() as u64 + 6)?;
        if base == 0 {
            return Ok(0);
        }
        mem.write_int(base, 4, bytes.len() as u64)?;
        mem.write(base + 4, bytes)?;
        mem.write(base + 4 + bytes.len() as u64, &[0; 2])?;
        self.track(base + 4, base, bytes.len());
        Ok(base + 4)
    }
    pub(crate) fn call<B: Backend>(
        &mut self,
        mem: &mut B,
        id: BstrApi,
        a: &[u64],
    ) -> Result<u64, B::Error> {
        use BstrApi::*;
        match id {
            Length | ByteLength => {
                if a[0] == 0 {
                    return Ok(0);
                }
                let length = mem.read_int(
                    a[0].checked_sub(4)
                        .ok_or_else(|| mem.invalid("BSTR prefix underflow"))?,
                    4,
                )?;
                Ok(if id == Length { length / 2 } else { length })
            }
            Free => {
                self.free(mem, a[0])?;
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
                self.allocate(mem, &bytes)
            }
            Reallocate | ReallocateLength => {
                if a[0] == 0 {
                    return Ok(0);
                }
                mem.check_write(a[0], 8)?;
                let old = mem.read_int(a[0], 8)?;
                self.validate_replacement(mem, old, a[0], 8)?;
                if id == Reallocate && a[1] == 0 {
                    self.free(mem, old)?;
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
                let next = self.allocate(mem, &bytes)?;
                if next == 0 {
                    return Ok(0);
                }
                self.free(mem, old)?;
                mem.write_int(a[0], 8, next)?;
                Ok(1)
            }
        }
    }
}

impl Bstrings {
    pub(crate) fn variant<B: Backend>(
        &mut self,
        mem: &mut B,
        id: VariantApi,
        a: &[u64],
    ) -> Result<u64, B::Error> {
        super::variant::call(
            &mut VariantAccess {
                strings: self,
                memory: mem,
            },
            id,
            a,
        )
    }
}
struct VariantAccess<'a, B> {
    strings: &'a mut Bstrings,
    memory: &'a mut B,
}
impl<B: Backend> super::variant::Backend for VariantAccess<'_, B> {
    type Error = B::Error;
    const SIZE: usize = 24;
    const POINTER: usize = 8;
    fn check(&self, p: u64, n: usize) -> Result<(), B::Error> {
        self.memory.check_write(p, n)
    }
    fn read(&self, p: u64, bytes: &mut [u8]) -> Result<(), B::Error> {
        self.memory.read(p, bytes)
    }
    fn write(&mut self, p: u64, bytes: &[u8]) -> Result<(), B::Error> {
        self.memory.write(p, bytes)
    }
    fn string(&mut self, source: Option<u64>) -> Result<u64, B::Error> {
        let bytes = match source {
            Some(0) => return Ok(0),
            Some(p) => {
                let prefix = p
                    .checked_sub(4)
                    .ok_or_else(|| B::windows(super::variant::INVALID))?;
                let length = self.memory.read_int(prefix, 4)? as u32;
                bstr_source(self.memory, p, Some(length))?.ok_or_else(|| B::windows(8))?
            }
            None => Vec::new(),
        };
        let p = self.strings.allocate(self.memory, &bytes)?;
        if p == 0 {
            return Err(B::windows(8));
        }
        Ok(p)
    }
    fn release(&mut self, p: u64, output: u64, size: usize) -> Result<(), B::Error> {
        if p == 0 {
            return Ok(());
        }
        if !self.strings.blocks.contains_key(&p) {
            return Err(B::windows(super::variant::INVALID));
        }
        self.strings
            .validate_replacement(self.memory, p, output, size)?;
        self.strings.free(self.memory, p)
    }
    fn scalar(&self, vt: u16) -> bool {
        matches!(vt, 0..=7 | 10..=11 | 14 | 16..=23)
    }
    fn windows(code: u32) -> B::Error {
        B::windows(code)
    }
}
