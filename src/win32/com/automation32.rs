/*
 * BSTR/VARIANT operations adapted from Hangover's Wine oleaut32/oleaut.c,
 * commit 3ea3a14941b6a122b892c84141ce9662c008081e.
 * Copyright 1999, 2000 Marcus Meissner
 * LGPL-2.1-or-later; see third_party/hangover/README.md.
 */
use crate::win32::catalog::{BstrApi, VariantApi};
use std::collections::HashMap;

pub(crate) trait Backend {
    type Error;
    // The legacy adapter translates memory faults to the specified Windows code.
    fn check(&self, p: u32, n: usize, write: bool, code: u32) -> Result<(), Self::Error>;
    fn read(&self, p: u32, bytes: &mut [u8], code: u32) -> Result<(), Self::Error>;
    fn write(&mut self, p: u32, bytes: &[u8], code: u32) -> Result<(), Self::Error>;
    // The shared process heap supplies zeroed payload and terminator storage.
    fn allocate(&mut self, n: u32) -> Result<u32, Self::Error>;
    fn check_release(&self, base: u32) -> Result<(), Self::Error>;
    fn free(&mut self, base: u32) -> Result<bool, Self::Error>;
    fn replacement(&self, _base: u32, _output: u32, _length: u32) -> Result<(), Self::Error> {
        Ok(())
    }
    fn windows(code: u32) -> Self::Error;
    fn scalar(&self, vt: u16) -> bool {
        matches!(vt, 0..=7 | 10..=14 | 16..=23)
    }
    fn integer(&self, p: u32, width: usize, code: u32) -> Result<u32, Self::Error> {
        let mut bytes = [0; 4];
        self.read(p, &mut bytes[..width], code)?;
        Ok(u32::from_le_bytes(bytes))
    }
    fn put(&mut self, p: u32, width: usize, value: u32, code: u32) -> Result<(), Self::Error> {
        self.write(p, &value.to_le_bytes()[..width], code)
    }
    fn units(&self, p: u32, length: u32) -> Result<Vec<u16>, Self::Error> {
        if p == 0 {
            return Err(Self::windows(87));
        }
        if length == u32::MAX {
            let mut text = Vec::new();
            for i in 0..32768u32 {
                let at = p.checked_add(i * 2).ok_or_else(|| Self::windows(87))?;
                let unit = self.integer(at, 2, 87)? as u16;
                if unit == 0 {
                    return Ok(text);
                }
                text.push(unit);
            }
            return Err(Self::windows(87));
        }
        if length > 1 << 20 {
            return Err(Self::windows(87));
        }
        let mut bytes = vec![0; length as usize * 2];
        self.read(p, &mut bytes, 87)?;
        Ok(bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect())
    }
}
#[derive(Default)]
pub(crate) struct Bstrings {
    blocks: HashMap<u32, u32>,
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
        p: u32,
        output: u32,
        size: u32,
    ) -> Result<(), B::Error> {
        if p != 0 {
            let base = *self
                .blocks
                .get(&p)
                .ok_or_else(|| B::windows(super::variant::INVALID))?;
            mem.check_release(base)?;
            mem.replacement(base, output, size)?;
        }
        Ok(())
    }
    pub(crate) fn free_string<B: Backend>(&mut self, mem: &mut B, p: u32) -> Result<(), B::Error> {
        if p != 0 {
            self.release(mem, p)?
                .ok_or_else(|| B::windows(super::variant::INVALID))?;
        }
        Ok(())
    }
    fn release_with<E>(
        &mut self,
        p: u32,
        free: impl FnOnce(u32) -> Result<bool, E>,
    ) -> Result<Option<bool>, E> {
        self.blocks.remove(&p).map(free).transpose()
    }
    fn release<B: Backend>(&mut self, mem: &mut B, p: u32) -> Result<Option<bool>, B::Error> {
        if let Some(&base) = self.blocks.get(&p) {
            mem.check_release(base)?;
        }
        self.release_with(p, |base| mem.free(base))
    }
    pub(crate) fn allocate<B: Backend>(
        &mut self,
        mem: &mut B,
        text: &[u16],
    ) -> Result<u32, B::Error> {
        self.allocate_bytes(
            mem,
            &text
                .iter()
                .flat_map(|u| u.to_le_bytes())
                .collect::<Vec<_>>(),
        )
    }
    fn allocate_bytes<B: Backend>(&mut self, mem: &mut B, bytes: &[u8]) -> Result<u32, B::Error> {
        let length = u32::try_from(bytes.len()).map_err(|_| B::windows(8))?;
        let base = mem.allocate(length.checked_add(6).ok_or_else(|| B::windows(8))?)?;
        if base == 0 {
            return Err(B::windows(8));
        }
        mem.put(base, 4, length, 87)?;
        mem.write(base + 4, bytes, 87)?;
        self.blocks.insert(base + 4, base);
        Ok(base + 4)
    }
    pub(crate) fn call<B: Backend>(
        &mut self,
        mem: &mut B,
        id: BstrApi,
        a: &[u32],
    ) -> Result<u32, B::Error> {
        use BstrApi::*;
        match id {
            Allocate | AllocateLength => {
                if id == Allocate && a[0] == 0 {
                    return Ok(0);
                }
                let text = if id == Allocate {
                    mem.units(a[0], u32::MAX)?
                } else if a[1] > 1 << 20 {
                    return Err(B::windows(8));
                } else if a[0] == 0 {
                    vec![0; a[1] as usize]
                } else {
                    mem.units(a[0], a[1])?
                };
                self.allocate(mem, &text)
            }
            Length | ByteLength => {
                if a[0] == 0 {
                    return Ok(0);
                }
                let length =
                    mem.integer(a[0].checked_sub(4).ok_or_else(|| B::windows(87))?, 4, 87)?;
                Ok(if id == Length { length / 2 } else { length })
            }
            Free => {
                if a[0] != 0 && !self.release(mem, a[0])?.ok_or_else(|| B::windows(87))? {
                    return Err(B::windows(87));
                }
                Ok(0)
            }
            Reallocate | ReallocateLength => {
                let old = mem.integer(a[0], 4, 87)?;
                mem.check(a[0], 4, true, 87)?;
                if old != 0 {
                    let base = self.blocks.get(&old).ok_or_else(|| B::windows(87))?;
                    mem.check_release(*base)?;
                    mem.replacement(*base, a[0], 4)?;
                }
                if id == Reallocate && a[1] == 0 {
                    mem.put(a[0], 4, 0, 87)?;
                    let _ = self.release(mem, old);
                    return Ok(1);
                }
                let length = id == ReallocateLength;
                let text = if a[1] == 0 || (length && old != 0 && a[1] == old) {
                    if length {
                        if a[2] > 1 << 20 {
                            return Err(B::windows(8));
                        }
                        let mut text = vec![0; a[2] as usize];
                        if old != 0 {
                            let old_length = mem.integer(old - 4, 4, 87)? / 2;
                            let prefix = mem.units(old, old_length.min(a[2]))?;
                            text[..prefix.len()].copy_from_slice(&prefix);
                        }
                        text
                    } else {
                        Vec::new()
                    }
                } else {
                    mem.units(a[1], if length { a[2] } else { u32::MAX })?
                };
                let pointer = self.allocate(mem, &text)?;
                mem.put(a[0], 4, pointer, 87)?;
                let _ = self.release(mem, old);
                Ok(1)
            }
            AllocateBytes => unreachable!("PE32 byte allocation remains ABI-only"),
        }
    }
    pub(crate) fn variant<B: Backend>(
        &mut self,
        mem: &mut B,
        id: VariantApi,
        a: &[u32],
    ) -> Result<u32, B::Error> {
        super::variant::call(
            &mut VariantAccess {
                strings: self,
                memory: mem,
            },
            id,
            &a.iter().map(|&v| u64::from(v)).collect::<Vec<_>>(),
        )
        .map(|v| v as u32)
    }
}
struct VariantAccess<'a, B> {
    strings: &'a mut Bstrings,
    memory: &'a mut B,
}
impl<B: Backend> super::variant::Backend for VariantAccess<'_, B> {
    type Error = B::Error;
    const SIZE: usize = 16;
    const POINTER: usize = 4;
    fn check(&self, p: u64, n: usize) -> Result<(), B::Error> {
        self.memory
            .check(address::<B>(p)?, n, true, super::variant::INVALID)
    }
    fn read(&self, p: u64, bytes: &mut [u8]) -> Result<(), B::Error> {
        self.memory
            .read(address::<B>(p)?, bytes, super::variant::INVALID)
    }
    fn write(&mut self, p: u64, bytes: &[u8]) -> Result<(), B::Error> {
        self.memory
            .write(address::<B>(p)?, bytes, super::variant::INVALID)
    }
    fn string(&mut self, source: Option<u64>) -> Result<u64, B::Error> {
        let bytes = match source {
            Some(0) => return Ok(0),
            Some(p) => {
                let p = address::<B>(p)?;
                let length = self.memory.integer(
                    p.checked_sub(4)
                        .ok_or_else(|| B::windows(super::variant::INVALID))?,
                    4,
                    super::variant::INVALID,
                )?;
                if length > 2 * 1024 * 1024 {
                    return Err(B::windows(8));
                }
                let mut bytes = vec![0; length as usize];
                self.memory.read(p, &mut bytes, super::variant::INVALID)?;
                bytes
            }
            None => Vec::new(),
        };
        self.strings
            .allocate_bytes(self.memory, &bytes)
            .map(u64::from)
    }
    fn release(&mut self, p: u64, output: u64, size: usize) -> Result<(), B::Error> {
        if p == 0 {
            return Ok(());
        }
        let p = address::<B>(p)?;
        let base = *self
            .strings
            .blocks
            .get(&p)
            .ok_or_else(|| B::windows(super::variant::INVALID))?;
        self.memory
            .replacement(base, address::<B>(output)?, size as u32)?;
        self.strings
            .release(self.memory, p)?
            .ok_or_else(|| B::windows(super::variant::INVALID))?;
        Ok(())
    }
    fn scalar(&self, vt: u16) -> bool {
        self.memory.scalar(vt)
    }
    fn windows(code: u32) -> B::Error {
        B::windows(code)
    }
}
fn address<B: Backend>(p: u64) -> Result<u32, B::Error> {
    u32::try_from(p).map_err(|_| B::windows(super::variant::INVALID))
}
