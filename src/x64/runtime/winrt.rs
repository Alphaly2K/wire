//! Checked Win64 HSTRINGs. The caller owns reference headers and their buffers.
//! Semantics reviewed against Hangover Wine string.c / roapi.c at
//! 3ea3a14941b6a122b892c84141ce9662c008081e, LGPL-2.1-or-later.
//! Copyright 2014 Martin Storsjo, 2016 Michael Müller (upstream reference).
//! No host Wine backend or pointers are used.
use super::{Memory, Runtime64, READ, WRITE};
use crate::win32::catalog::WinrtApi;
mod strings;
use strings::Retention;
pub(super) use strings::Strings;
const INVALIDARG: u64 = 0x80070057;
const OUTOFMEMORY: u64 = 0x8007000e;
const MAX_UNITS: u32 = 4 * 1024 * 1024;
impl Runtime64 {
    fn hstring_copy(&mut self, mem: &mut Memory, bytes: &[u8]) -> Result<u64, String> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let length = bytes.len() / 2;
        if !self.hstrings.can_copy(length) {
            return Ok(0);
        }
        let handle = self.heap_alloc(mem, bytes.len() as u64 + 34)?;
        if handle != 0 {
            mem.write_int(handle + 4, 4, length as u64)?;
            mem.write_int(handle + 16, 8, handle + 32)?;
            mem.write(handle + 32, bytes)?;
            mem.protect(handle, self.allocations[&handle].1, READ)?;
            self.hstrings.track(handle, length as u32);
        }
        Ok(handle)
    }
    pub(super) fn winrt_call(
        &mut self,
        mem: &mut Memory,
        id: WinrtApi,
        a: &[u64],
    ) -> Result<u64, String> {
        use WinrtApi::*;
        match id {
            CreateString | CreateReference => {
                let reference = id == CreateReference;
                let out = a[if reference { 3 } else { 2 }];
                if out == 0 || (reference && a[2] == 0) {
                    return Ok(INVALIDARG);
                }
                mem.check(out, 8, WRITE)?;
                let length = a[1] as u32;
                if length > MAX_UNITS {
                    return Ok(OUTOFMEMORY);
                }
                if reference && a[0] != 0 {
                    mem.check(a[0], (length as usize + 1) * 2, READ)?;
                    if mem.read_int(a[0] + length as u64 * 2, 2)? != 0 {
                        return Ok(INVALIDARG);
                    }
                }
                if length == 0 {
                    mem.write_int(out, 8, 0)?;
                    return Ok(0);
                }
                if a[0] == 0 {
                    return Ok(0x80004003);
                }
                if reference {
                    mem.check(a[2], 24, WRITE)?;
                    if self.hstrings.owns(a[2]) {
                        return Ok(INVALIDARG);
                    }
                    // Padding is opaque. Preflight all outputs before changing
                    // the header or publishing a reference to caller storage.
                    mem.write_int(a[2], 4, 1)?;
                    mem.write_int(a[2] + 4, 4, length as u64)?;
                    mem.write_int(a[2] + 16, 8, a[0])?;
                    mem.write_int(out, 8, a[2])?;
                    Ok(0)
                } else {
                    let mut bytes = vec![0; length as usize * 2];
                    mem.read(a[0], &mut bytes)?;
                    let handle = self.hstring_copy(mem, &bytes)?;
                    if handle == 0 {
                        return Ok(OUTOFMEMORY);
                    }
                    mem.write_int(out, 8, handle)?;
                    Ok(0)
                }
            }
            DeleteString => {
                self.hstrings.release(mem, &mut self.allocations, a[0])?;
                Ok(0)
            }
            DuplicateString => {
                if a[1] == 0 {
                    return Ok(INVALIDARG);
                }
                mem.check(a[1], 8, WRITE)?;
                match self.hstrings.retain(a[0]) {
                    Retention::Overflow => return Ok(OUTOFMEMORY),
                    Retention::Retained => mem.write_int(a[1], 8, a[0])?,
                    Retention::Reference => {
                        let bytes = self.hstrings.bytes(mem, a[0])?;
                        let handle = self.hstring_copy(mem, &bytes)?;
                        if handle == 0 && !bytes.is_empty() {
                            return Ok(OUTOFMEMORY);
                        }
                        mem.write_int(a[1], 8, handle)?;
                    }
                }
                Ok(0)
            }
            StringLength | IsEmpty | RawBuffer => {
                let (mut pointer, length) = self.hstrings.parts(mem, a[0])?;
                if id == StringLength {
                    return Ok(length as u64);
                }
                if id == IsEmpty {
                    return Ok((length == 0) as u64);
                }
                if a[1] != 0 {
                    mem.check(a[1], 4, WRITE)?;
                }
                if pointer == 0 {
                    if self.hstrings.empty_buffer() == 0 {
                        let p = self.heap_alloc(mem, 2)?;
                        if p == 0 {
                            return Err("Win64 empty HSTRING allocation failed".into());
                        }
                        mem.protect(p, self.allocations[&p].1, READ)?;
                        self.hstrings.cache_empty_buffer(p);
                    }
                    pointer = self.hstrings.empty_buffer();
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
                mem.check(out, 4, WRITE)?;
                let left = self.hstrings.bytes(mem, a[0])?;
                let result = if id == HasEmbeddedNull {
                    left.as_chunks::<2>().0.contains(&[0, 0]) as i32
                } else {
                    let right = self.hstrings.bytes(mem, a[1])?;
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
                mem.check(out, 8, WRITE)?;
                if factory {
                    mem.check(a[1], 16, READ)?;
                }
                let bytes = self.hstrings.bytes(mem, a[0])?;
                let units: Vec<_> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes(*b))
                    .collect();
                self.trace_note(format!(
                    "Win64 WinRT class={:?}: no registered activation factory",
                    String::from_utf16_lossy(&units)
                ));
                // There are no installed activatable classes. Preserve the
                // output on registry lookup failure, as the reference does.
                Ok(0x80040154)
            }
        }
    }
}
