/*
 * BSTR operations adapted from Hangover's Wine oleaut32/oleaut.c,
 * commit 3ea3a14941b6a122b892c84141ce9662c008081e, 2026-10-06.
 * Copyright 1999, 2000 Marcus Meissner
 * This library is free software; you can redistribute it and/or modify it
 * under the GNU Lesser General Public License, version 2.1 or (at your
 * option) any later version. No warranty; see third_party/hangover/README.md.
 */
//! Common process/string/COM allocation contracts. Complex COM objects and
//! locale collation still require their respective backends; no fake success.
use crate::api::{ApiState, Args, HandleObj};
use crate::mem::GuestMem;
use crate::thread::Sched;
use crate::win32::text::units32 as units;
use std::cmp::Ordering;
fn guid_text(mem: &GuestMem, p: u32) -> Result<String, u32> {
    let mut b = [0; 16];
    mem.read_bytes(p, &mut b).map_err(|_| 0x80004003u32)?;
    Ok(crate::win32::com::guid::format_text(b))
}
pub(crate) fn allocate_bstr(
    mem: &mut GuestMem,
    state: &mut ApiState,
    text: &[u16],
) -> Result<u32, u32> {
    state
        .bstrs
        .allocate(mem, &mut state.heap, state.process_heap, text)
}

pub(crate) fn dispatch(
    dll: &str,
    name: &str,
    a: &Args,
    mem: &mut GuestMem,
    state: &mut ApiState,
    sched: &Sched,
) -> Option<(u32, String, bool)> {
    if !(dll == "kernel32.dll"
        && matches!(
            name,
            "CompareStringOrdinal"
                | "lstrcmpW"
                | "lstrcmpA"
                | "lstrcmpiW"
                | "lstrcmpiA"
                | "lstrlenA"
                | "lstrlenW"
                | "lstrlen"
                | "lstrcpyA"
                | "lstrcpyW"
                | "lstrcatA"
                | "lstrcatW"
                | "lstrcpynA"
                | "lstrcpynW"
                | "IsBadReadPtr"
                | "IsBadWritePtr"
                | "IsBadCodePtr"
                | "FlushInstructionCache"
                | "MulDiv"
                | "OpenProcess"
                | "GetProcessAffinityMask"
                | "SetProcessAffinityMask"
                | "SetThreadIdealProcessor"
                | "GetProcessId"
                | "GetExitCodeProcess"
                | "GetActiveProcessorCount"
                | "GetMaximumProcessorCount"
        ))
        && !(dll == "ole32.dll"
            && matches!(name, "CoCreateGuid" | "StringFromCLSID" | "StringFromIID"))
    {
        return None;
    }
    let result = (|| -> Result<u32, u32> {
        match name {
            "lstrlen" | "lstrlenA" | "lstrlenW" | "lstrcpyA" | "lstrcpyW" | "lstrcatA"
            | "lstrcatW" | "lstrcpynA" | "lstrcpynW" => {
                let wide = name.ends_with('W');
                let unit = if wide { 2 } else { 1 };
                let read = |p: u32, n: u32, truncated: bool| -> Result<Vec<u8>, u32> {
                    let mut bytes = Vec::new();
                    for i in 0..n.min(1 << 20) {
                        let address = p.checked_add(i * unit).ok_or(87u32)?;
                        let c = if wide {
                            mem.read_u16(address).map_err(|_| 87u32)?
                        } else {
                            mem.read_u8(address).map_err(|_| 87u32)? as u16
                        };
                        if c == 0 {
                            return Ok(bytes);
                        }
                        if wide {
                            bytes.extend(c.to_le_bytes());
                        } else {
                            bytes.push(c as u8);
                        }
                    }
                    if truncated && n <= 1 << 20 {
                        Ok(bytes)
                    } else {
                        Err(87)
                    }
                };
                if name.starts_with("lstrlen") {
                    if a[0] == 0 {
                        return Ok(0);
                    }
                    return Ok(read(a[0], 1 << 20, false)?.len() as u32 / unit);
                }
                let limited = name.starts_with("lstrcpyn");
                if limited && (a[2] as i32) <= 0 {
                    return Ok(a[0]);
                }
                let capacity = if limited { a[2] - 1 } else { 1 << 20 };
                let mut bytes = read(a[1], capacity, limited)?;
                bytes.extend(vec![0; unit as usize]);
                let offset = if name.starts_with("lstrcat") {
                    read(a[0], 1 << 20, false)?.len() as u32
                } else {
                    0
                };
                mem.write_bytes(a[0].checked_add(offset).ok_or(87u32)?, &bytes)
                    .map_err(|_| 87u32)?;
                Ok(a[0])
            }
            "OpenProcess" => {
                if a[2] != 4242 {
                    return Err(if a[2] == 0 { 87 } else { 5 });
                }
                Ok(state.alloc_handle(HandleObj::Process { access: a[0] }))
            }
            "GetProcessAffinityMask"
            | "SetProcessAffinityMask"
            | "GetProcessId"
            | "GetExitCodeProcess" => {
                if a[0] != u32::MAX {
                    let Some(HandleObj::Process { access }) = state.handles.get(&a[0]) else {
                        return Err(6);
                    };
                    let required = if name == "SetProcessAffinityMask" {
                        0x200
                    } else {
                        0x1400
                    };
                    if access & required == 0 {
                        return Err(5);
                    }
                }
                match name {
                    "GetProcessId" => Ok(4242),
                    "GetExitCodeProcess" => {
                        mem.write_u32(a[1], 259).map_err(|_| 87u32)?;
                        Ok(1)
                    }
                    "SetProcessAffinityMask" => {
                        if a[1] == 1 {
                            Ok(1)
                        } else {
                            Err(87)
                        }
                    }
                    _ => {
                        mem.ptr(a[1], 4, true).map_err(|_| 87u32)?;
                        mem.ptr(a[2], 4, true).map_err(|_| 87u32)?;
                        mem.write_u32(a[1], 1).map_err(|_| 87u32)?;
                        mem.write_u32(a[2], 1).map_err(|_| 87u32)?;
                        Ok(1)
                    }
                }
            }
            "SetThreadIdealProcessor" => {
                if a[0] != 0xfffffffe && sched.thread_index_by_handle(a[0]).is_none() {
                    return Err(6);
                }
                if !matches!(a[1], 0 | u32::MAX) {
                    return Err(87);
                }
                Ok(0)
            }
            "GetActiveProcessorCount" | "GetMaximumProcessorCount" => {
                if !matches!(a[0], 0 | 0xffff) {
                    return Err(87);
                }
                Ok(1)
            }
            "CompareStringOrdinal" | "lstrcmpW" | "lstrcmpA" | "lstrcmpiW" | "lstrcmpiA" => {
                let ordinal = name == "CompareStringOrdinal";
                let wide = ordinal || name.ends_with('W');
                let mut left = if ordinal {
                    units(mem, a[0], a[1])?
                } else {
                    crate::menu::read_text(mem, a[0], wide)?.unwrap_or_default()
                };
                let mut right = if ordinal {
                    units(mem, a[2], a[3])?
                } else {
                    crate::menu::read_text(mem, a[1], wide)?.unwrap_or_default()
                };
                if (ordinal && a[4] != 0) || name.starts_with("lstrcmpi") {
                    for text in [&mut left, &mut right] {
                        for c in text {
                            if let Some(ch) = char::from_u32(*c as u32) {
                                let mut uppercase = ch.to_uppercase();
                                let n = uppercase.next().unwrap();
                                if uppercase.next().is_none() && n as u32 <= 65535 {
                                    *c = n as u16;
                                }
                            }
                        }
                    }
                }
                let result = match left.cmp(&right) {
                    Ordering::Less => -1i32,
                    Ordering::Equal => 0,
                    Ordering::Greater => 1,
                };
                Ok(if ordinal {
                    (result + 2) as u32
                } else {
                    result as u32
                })
            }
            "IsBadReadPtr" | "IsBadWritePtr" => {
                Ok((a[1] != 0 && mem.ptr(a[0], a[1], name == "IsBadWritePtr").is_err()) as u32)
            }
            "IsBadCodePtr" => Ok(mem.inspect_code(a[0], &mut [0]).is_err() as u32),
            "FlushInstructionCache" => {
                if a[0] != u32::MAX {
                    return Err(6);
                }
                Ok(1)
            } // IR already invalidated by every code write
            "MulDiv" => {
                let divisor = a[2] as i32 as i64;
                if divisor == 0 {
                    return Ok(u32::MAX);
                }
                let n = (a[0] as i32 as i64) * (a[1] as i32 as i64);
                let negative = (n < 0) != (divisor < 0);
                let quotient =
                    (n.unsigned_abs() + divisor.unsigned_abs() / 2) / divisor.unsigned_abs();
                let result = if negative {
                    -(quotient as i64)
                } else {
                    quotient as i64
                };
                Ok(i32::try_from(result).map_or(u32::MAX, |n| n as u32))
            }
            "StringFromCLSID" | "StringFromIID" => {
                let text = guid_text(mem, a[0])?;
                let mut bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
                bytes.extend([0, 0]);
                mem.write_u32(a[1], 0).map_err(|_| 0x80004003u32)?;
                let p = state
                    .heap
                    .alloc(mem, state.process_heap, 78, false)
                    .map_err(|_| 0x8007000eu32)?;
                if p == 0 {
                    return Err(0x8007000e);
                }
                if mem.write_bytes(p, &bytes).is_err() || mem.write_u32(a[1], p).is_err() {
                    state.heap.free(state.process_heap, p);
                    return Err(0x80004003);
                }
                Ok(0)
            }
            "CoCreateGuid" => {
                mem.ptr(a[0], 16, true).map_err(|_| 0x80004003u32)?;
                let mut guid = [0; 16];
                if unsafe { libc::getentropy(guid.as_mut_ptr().cast(), guid.len()) } != 0 {
                    return Err(0x80004005);
                }
                guid[7] = (guid[7] & 15) | 0x40;
                guid[8] = (guid[8] & 63) | 0x80;
                mem.write_bytes(a[0], &guid).map_err(|_| 0x80004003u32)?;
                Ok(0)
            }
            _ => unreachable!(),
        }
    })();
    Some(match result {
        Ok(value) => (
            value,
            "common process/string/COM allocation contract".into(),
            true,
        ),
        Err(error) => {
            state.last_error = error;
            (
                if dll == "ole32.dll" {
                    error
                } else if name == "SetThreadIdealProcessor" {
                    u32::MAX
                } else {
                    0
                },
                format!("common API error {error:#x}"),
                true,
            )
        }
    })
}
