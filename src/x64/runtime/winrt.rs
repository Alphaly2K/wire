//! Checked Win64 HSTRINGs. The caller owns reference headers and their buffers.
//! Semantics reviewed against Hangover Wine string.c / roapi.c at
//! 3ea3a14941b6a122b892c84141ce9662c008081e, LGPL-2.1-or-later.
//! Copyright 2014 Martin Storsjo, 2016 Michael Müller (upstream reference).
//! No host Wine backend or pointers are used.
use super::{Memory, Runtime64, READ, WRITE};
use std::collections::BTreeMap;
const INVALIDARG: u64 = 0x80070057;
const OUTOFMEMORY: u64 = 0x8007000e;
const MAX_UNITS: u32 = 4 * 1024 * 1024;
#[derive(Default)]
pub(super) struct Strings {
    owned: BTreeMap<u64, (u32, u32)>, // length, references
    empty: u64,
}
pub(super) fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "WindowsCreateStringReference" => 4,
        "WindowsCreateString" | "WindowsCompareStringOrdinal" | "RoGetActivationFactory" => 3,
        "WindowsDuplicateString"
        | "WindowsGetStringRawBuffer"
        | "WindowsStringHasEmbeddedNull"
        | "RoActivateInstance" => 2,
        "WindowsDeleteString" | "WindowsGetStringLen" | "WindowsIsStringEmpty" => 1,
        _ => return None,
    })
}
pub(super) fn export(dll: &str, name: &str) -> bool {
    arity(name).is_some()
        && (dll.eq_ignore_ascii_case("combase.dll")
            || (name.starts_with("Windows")
                && dll.eq_ignore_ascii_case("api-ms-win-core-winrt-string-l1-1-0.dll"))
            || (name.starts_with("Ro")
                && dll.eq_ignore_ascii_case("api-ms-win-core-winrt-l1-1-0.dll")))
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    match name {
        "WindowsCreateString" | "WindowsCreateStringReference" => &[1],
        _ => &[],
    }
}
impl Runtime64 {
    fn hstring_parts(&self, mem: &Memory, handle: u64) -> Result<(u64, u32), String> {
        if handle == 0 {
            return Ok((0, 0));
        }
        if let Some(&(length, _)) = self.hstrings.owned.get(&handle) {
            mem.check(handle, 24, READ)?;
            return Ok((handle + 32, length));
        }
        // Reference handles are the guest's 24-byte HSTRING_HEADER. Read it
        // again to honor the caller's lifetime; no stale host metadata survives.
        let mut header = [0; 24];
        mem.read(handle, &mut header)?;
        let flags = u32::from_le_bytes(header[..4].try_into().unwrap());
        let length = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let pointer = u64::from_le_bytes(header[16..24].try_into().unwrap());
        if flags != 1 || length > MAX_UNITS || pointer == 0 {
            return Err("invalid Win64 HSTRING reference header".into());
        }
        mem.check(pointer, (length as usize + 1) * 2, READ)?;
        Ok((pointer, length))
    }
    fn hstring_bytes(&self, mem: &Memory, handle: u64) -> Result<Vec<u8>, String> {
        let (pointer, length) = self.hstring_parts(mem, handle)?;
        let mut data = vec![0; length as usize * 2];
        mem.read(pointer, &mut data)?;
        Ok(data)
    }
    fn hstring_copy(&mut self, mem: &mut Memory, bytes: &[u8]) -> Result<u64, String> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let length = bytes.len() / 2;
        if length > MAX_UNITS as usize || self.hstrings.owned.len() >= 4096 {
            return Ok(0);
        }
        let handle = self.heap_alloc(mem, bytes.len() as u64 + 34)?;
        if handle != 0 {
            mem.write_int(handle + 4, 4, length as u64)?;
            mem.write_int(handle + 16, 8, handle + 32)?;
            mem.write(handle + 32, bytes)?;
            mem.protect(handle, self.allocations[&handle].1, READ)?;
            self.hstrings.owned.insert(handle, (length as u32, 1));
        }
        Ok(handle)
    }
    pub(super) fn winrt_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        a: &[u64],
    ) -> Result<u64, String> {
        match name {
            "WindowsCreateString" | "WindowsCreateStringReference" => {
                let reference = name.ends_with("Reference");
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
                    if self.hstrings.owned.contains_key(&a[2]) {
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
            "WindowsDeleteString" => {
                if let Some(&(_, refs)) = self.hstrings.owned.get(&a[0]) {
                    if refs == 1 {
                        mem.unmap(a[0], self.allocations[&a[0]].1)?;
                        self.allocations.remove(&a[0]);
                        self.hstrings.owned.remove(&a[0]);
                    } else {
                        self.hstrings.owned.get_mut(&a[0]).unwrap().1 -= 1;
                    }
                } else {
                    self.hstring_parts(mem, a[0])?;
                }
                Ok(0)
            }
            "WindowsDuplicateString" => {
                if a[1] == 0 {
                    return Ok(INVALIDARG);
                }
                mem.check(a[1], 8, WRITE)?;
                if let Some((_, refs)) = self.hstrings.owned.get_mut(&a[0]) {
                    let Some(next) = refs.checked_add(1) else {
                        return Ok(OUTOFMEMORY);
                    };
                    *refs = next;
                    mem.write_int(a[1], 8, a[0])?;
                } else {
                    let bytes = self.hstring_bytes(mem, a[0])?;
                    let handle = self.hstring_copy(mem, &bytes)?;
                    if handle == 0 && !bytes.is_empty() {
                        return Ok(OUTOFMEMORY);
                    }
                    mem.write_int(a[1], 8, handle)?;
                }
                Ok(0)
            }
            "WindowsGetStringLen" | "WindowsIsStringEmpty" | "WindowsGetStringRawBuffer" => {
                let (mut pointer, length) = self.hstring_parts(mem, a[0])?;
                if name == "WindowsGetStringLen" {
                    return Ok(length as u64);
                }
                if name == "WindowsIsStringEmpty" {
                    return Ok((length == 0) as u64);
                }
                if a[1] != 0 {
                    mem.check(a[1], 4, WRITE)?;
                }
                if pointer == 0 {
                    if self.hstrings.empty == 0 {
                        let p = self.heap_alloc(mem, 2)?;
                        if p == 0 {
                            return Err("Win64 empty HSTRING allocation failed".into());
                        }
                        mem.protect(p, self.allocations[&p].1, READ)?;
                        self.hstrings.empty = p;
                    }
                    pointer = self.hstrings.empty;
                }
                if a[1] != 0 {
                    mem.write_int(a[1], 4, length as u64)?;
                }
                Ok(pointer)
            }
            "WindowsStringHasEmbeddedNull" | "WindowsCompareStringOrdinal" => {
                let out = a[if name == "WindowsStringHasEmbeddedNull" {
                    1
                } else {
                    2
                }];
                if out == 0 {
                    return Ok(INVALIDARG);
                }
                mem.check(out, 4, WRITE)?;
                let left = self.hstring_bytes(mem, a[0])?;
                let result = if name == "WindowsStringHasEmbeddedNull" {
                    left.as_chunks::<2>().0.contains(&[0, 0]) as i32
                } else {
                    let right = self.hstring_bytes(mem, a[1])?;
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
            "RoGetActivationFactory" | "RoActivateInstance" => {
                let factory = name == "RoGetActivationFactory";
                let out = a[if factory { 2 } else { 1 }];
                if out == 0 || (factory && a[1] == 0) {
                    return Ok(INVALIDARG);
                }
                mem.check(out, 8, WRITE)?;
                if factory {
                    mem.check(a[1], 16, READ)?;
                }
                let bytes = self.hstring_bytes(mem, a[0])?;
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
            _ => unreachable!(),
        }
    }
}
