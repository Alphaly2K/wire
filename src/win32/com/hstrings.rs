//! Checked HSTRINGs with portable ownership and memory adapters. The caller owns reference headers and their buffers.
//! Semantics reviewed against Hangover Wine string.c / roapi.c at
//! 3ea3a14941b6a122b892c84141ce9662c008081e, LGPL-2.1-or-later.
//! Copyright 2014 Martin Storsjo, 2016 Michael Müller (upstream reference).
//! No host Wine backend or pointers are used.
use crate::win32::catalog::WinrtApi;
use std::collections::BTreeMap;

pub(crate) trait Backend {
    type Error;
    fn width(&self) -> usize;
    fn check(&self, address: u64, length: usize, write: bool) -> Result<(), Self::Error>;
    fn read(&self, address: u64, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Self::Error>;
    fn allocate(&mut self, size: u64) -> Result<u64, Self::Error>;
    fn readonly(&mut self, address: u64) -> Result<(), Self::Error>;
    fn free(&mut self, address: u64) -> Result<(), Self::Error>;
    fn invalid(&self, message: &'static str) -> Self::Error;
    fn activation(&mut self, units: &[u16]);
    fn read_int(&self, address: u64, width: usize) -> Result<u64, Self::Error> {
        let mut bytes = [0; 8];
        self.read(address, &mut bytes[..width])?;
        Ok(u64::from_le_bytes(bytes))
    }
    fn write_int(&mut self, address: u64, width: usize, value: u64) -> Result<(), Self::Error> {
        self.write(address, &value.to_le_bytes()[..width])
    }
}
const INVALIDARG: u64 = 0x80070057;
const OUTOFMEMORY: u64 = 0x8007000e;
const MAX_UNITS: u32 = 4 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct Strings {
    owned: BTreeMap<u64, (u32, u32)>, // length, references
    empty: u64,
}

pub(crate) enum Retention {
    Reference,
    Retained,
    Overflow,
}

impl Strings {
    pub(crate) fn parts<B: Backend>(&self, mem: &B, handle: u64) -> Result<(u64, u32), B::Error> {
        if handle == 0 {
            return Ok((0, 0));
        }
        if let Some(&(length, _)) = self.owned.get(&handle) {
            mem.check(handle, 24, false)?;
            return Ok((handle + 32, length));
        }
        // Caller storage can change between calls; never cache reference metadata.
        let mut header = [0; 24];
        let at = if mem.width() == 4 { 12 } else { 16 };
        mem.read(handle, &mut header[..at + mem.width()])?;
        let flags = u32::from_le_bytes(header[..4].try_into().unwrap());
        let length = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let mut pointer = [0; 8];
        pointer[..mem.width()].copy_from_slice(&header[at..at + mem.width()]);
        let pointer = u64::from_le_bytes(pointer);
        if flags != 1 || length > MAX_UNITS || pointer == 0 {
            return Err(mem.invalid("invalid HSTRING reference header"));
        }
        mem.check(pointer, (length as usize + 1) * 2, false)?;
        Ok((pointer, length))
    }

    pub(crate) fn bytes<B: Backend>(&self, mem: &B, handle: u64) -> Result<Vec<u8>, B::Error> {
        let (pointer, length) = self.parts(mem, handle)?;
        let mut data = vec![0; length as usize * 2];
        mem.read(pointer, &mut data)?;
        Ok(data)
    }

    pub(crate) fn can_copy(&self, length: usize) -> bool {
        length <= MAX_UNITS as usize && self.owned.len() < 4096
    }

    pub(crate) fn track(&mut self, handle: u64, length: u32) {
        self.owned.insert(handle, (length, 1));
    }

    pub(crate) fn owns(&self, handle: u64) -> bool {
        self.owned.contains_key(&handle)
    }

    pub(crate) fn release<B: Backend>(&mut self, mem: &mut B, handle: u64) -> Result<(), B::Error> {
        if let Some(&(_, refs)) = self.owned.get(&handle) {
            if refs == 1 {
                mem.free(handle)?;
                self.owned.remove(&handle);
            } else {
                self.owned.get_mut(&handle).unwrap().1 -= 1;
            }
        } else {
            self.parts(mem, handle)?;
        }
        Ok(())
    }

    pub(crate) fn retain(&mut self, handle: u64) -> Retention {
        let Some((_, refs)) = self.owned.get_mut(&handle) else {
            return Retention::Reference;
        };
        let Some(next) = refs.checked_add(1) else {
            return Retention::Overflow;
        };
        *refs = next;
        Retention::Retained
    }

    pub(crate) fn empty_buffer(&self) -> u64 {
        self.empty
    }

    pub(crate) fn cache_empty_buffer(&mut self, pointer: u64) {
        self.empty = pointer;
    }
}

impl Strings {
    fn copy<B: Backend>(&mut self, mem: &mut B, bytes: &[u8]) -> Result<u64, B::Error> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let length = bytes.len() / 2;
        if !self.can_copy(length) {
            return Ok(0);
        }
        let handle = mem.allocate(bytes.len() as u64 + 34)?;
        if handle != 0 {
            mem.write_int(handle + 4, 4, length as u64)?;
            mem.write_int(
                handle + if mem.width() == 4 { 12 } else { 16 },
                mem.width(),
                handle + 32,
            )?;
            mem.write(handle + 32, bytes)?;
            mem.readonly(handle)?;
            self.track(handle, length as u32);
        }
        Ok(handle)
    }
    pub(crate) fn invoke<B: Backend>(
        &mut self,
        mem: &mut B,
        id: WinrtApi,
        a: &[u64],
    ) -> Result<u64, B::Error> {
        use WinrtApi::*;
        match id {
            CreateString | CreateReference => {
                let reference = id == CreateReference;
                let out = a[if reference { 3 } else { 2 }];
                if out == 0 || (reference && a[2] == 0) {
                    return Ok(INVALIDARG);
                }
                mem.check(out, mem.width(), true)?;
                let length = a[1] as u32;
                if length > MAX_UNITS {
                    return Ok(OUTOFMEMORY);
                }
                if reference && a[0] != 0 {
                    mem.check(a[0], (length as usize + 1) * 2, false)?;
                    if mem.read_int(a[0] + length as u64 * 2, 2)? != 0 {
                        return Ok(INVALIDARG);
                    }
                }
                if length == 0 {
                    mem.write_int(out, mem.width(), 0)?;
                    return Ok(0);
                }
                if a[0] == 0 {
                    return Ok(0x80004003);
                }
                if reference {
                    mem.check(a[2], 24, true)?;
                    if self.owns(a[2]) {
                        return Ok(INVALIDARG);
                    }
                    // Padding is opaque. Preflight all outputs before changing
                    // the header or publishing a reference to caller storage.
                    mem.write_int(a[2], 4, 1)?;
                    mem.write_int(a[2] + 4, 4, length as u64)?;
                    mem.write_int(
                        a[2] + if mem.width() == 4 { 12 } else { 16 },
                        mem.width(),
                        a[0],
                    )?;
                    mem.write_int(out, mem.width(), a[2])?;
                    Ok(0)
                } else {
                    let mut bytes = vec![0; length as usize * 2];
                    mem.read(a[0], &mut bytes)?;
                    let handle = self.copy(mem, &bytes)?;
                    if handle == 0 {
                        return Ok(OUTOFMEMORY);
                    }
                    mem.write_int(out, mem.width(), handle)?;
                    Ok(0)
                }
            }
            DeleteString => {
                self.release(mem, a[0])?;
                Ok(0)
            }
            DuplicateString => {
                if a[1] == 0 {
                    return Ok(INVALIDARG);
                }
                mem.check(a[1], mem.width(), true)?;
                match self.retain(a[0]) {
                    Retention::Overflow => return Ok(OUTOFMEMORY),
                    Retention::Retained => mem.write_int(a[1], mem.width(), a[0])?,
                    Retention::Reference => {
                        let bytes = self.bytes(mem, a[0])?;
                        let handle = self.copy(mem, &bytes)?;
                        if handle == 0 && !bytes.is_empty() {
                            return Ok(OUTOFMEMORY);
                        }
                        mem.write_int(a[1], mem.width(), handle)?;
                    }
                }
                Ok(0)
            }
            StringLength | IsEmpty | RawBuffer => {
                let (mut pointer, length) = self.parts(mem, a[0])?;
                if id == StringLength {
                    return Ok(length as u64);
                }
                if id == IsEmpty {
                    return Ok((length == 0) as u64);
                }
                if a[1] != 0 {
                    mem.check(a[1], 4, true)?;
                }
                if pointer == 0 {
                    if self.empty_buffer() == 0 {
                        let p = mem.allocate(2)?;
                        if p == 0 {
                            return Err(mem.invalid("empty HSTRING allocation failed"));
                        }
                        mem.readonly(p)?;
                        self.cache_empty_buffer(p);
                    }
                    pointer = self.empty_buffer();
                }
                if a[1] != 0 {
                    mem.write_int(a[1], 4, length as u64)?;
                }
                Ok(pointer)
            }
            HasEmbeddedNull | CompareOrdinal => {
                let out = a[if id == HasEmbeddedNull { 1 } else { 2 }];
                if out == 0 {
                    return Ok(INVALIDARG);
                }
                mem.check(out, 4, true)?;
                let left = self.bytes(mem, a[0])?;
                let result = if id == HasEmbeddedNull {
                    left.as_chunks::<2>().0.contains(&[0, 0]) as i32
                } else {
                    let right = self.bytes(mem, a[1])?;
                    let units = |b: [u8; 2]| u16::from_le_bytes(b);
                    match left
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .copied()
                        .map(units)
                        .cmp(right.as_chunks::<2>().0.iter().copied().map(units))
                    {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Greater => 1,
                    }
                };
                mem.write_int(out, 4, result as u32 as u64)?;
                Ok(0)
            }
            ActivationFactory | ActivateInstance => {
                let factory = id == ActivationFactory;
                let out = a[if factory { 2 } else { 1 }];
                if out == 0 || (factory && a[1] == 0) {
                    return Ok(INVALIDARG);
                }
                mem.check(out, mem.width(), true)?;
                if factory {
                    mem.check(a[1], 16, false)?;
                }
                let bytes = self.bytes(mem, a[0])?;
                let units: Vec<_> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes(*b))
                    .collect();
                mem.activation(&units);
                // There are no installed activatable classes. Preserve the
                // output on registry lookup failure, as the reference does.
                Ok(0x80040154)
            }
        }
    }
}
