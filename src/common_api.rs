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
use std::cmp::Ordering;
fn units(mem: &GuestMem, p: u32, len: u32) -> Result<Vec<u16>, u32> {
    if p == 0 {
        return Err(87);
    }
    if len == u32::MAX {
        return Ok(crate::menu::read_text(mem, p, true)?.unwrap_or_default());
    }
    if len > 1 << 20 {
        return Err(87);
    }
    let mut bytes = vec![0; len as usize * 2];
    mem.read_bytes(p, &mut bytes).map_err(|_| 87u32)?;
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect())
}
fn allocate_bstr(mem: &mut GuestMem, state: &mut ApiState, text: &[u16]) -> Result<u32, u32> {
    let length = u32::try_from(text.len())
        .map_err(|_| 8u32)?
        .checked_mul(2)
        .ok_or(8u32)?;
    let base = state
        .heap
        .alloc(
            mem,
            state.process_heap,
            length.checked_add(6).ok_or(8u32)?,
            true,
        )
        .map_err(|_| 8u32)?;
    mem.write_u32(base, length).map_err(|_| 87u32)?;
    mem.write_bytes(
        base + 4,
        &text
            .iter()
            .flat_map(|c| c.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .map_err(|_| 87u32)?;
    state.bstrs.insert(base + 4, base);
    Ok(base + 4)
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
                | "IsBadReadPtr"
                | "IsBadWritePtr"
                | "IsBadCodePtr"
                | "FlushInstructionCache"
                | "MulDiv"
                | "OpenProcess"
                | "GetProcessAffinityMask"
                | "SetProcessAffinityMask"
                | "SetThreadAffinityMask"
                | "SetThreadIdealProcessor"
                | "GetProcessId"
                | "GetExitCodeProcess"
                | "GetActiveProcessorCount"
                | "GetMaximumProcessorCount"
        ))
        && !(dll == "ole32.dll"
            && matches!(
                name,
                "CoCreateGuid" | "CoInitialize" | "CoInitializeEx" | "CoUninitialize"
            ))
        && !(dll == "oleaut32.dll"
            && matches!(
                name,
                "SysAllocString"
                    | "SysAllocStringLen"
                    | "SysFreeString"
                    | "SysStringLen"
                    | "SysStringByteLen"
                    | "SysReAllocString"
                    | "SysReAllocStringLen"
            ))
    {
        return None;
    }
    let tid = sched.current_tid();
    let result = (|| -> Result<u32, u32> {
        match name {
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
            "SetThreadAffinityMask" | "SetThreadIdealProcessor" => {
                if a[0] != 0xfffffffe && sched.thread_index_by_handle(a[0]).is_none() {
                    return Err(6);
                }
                let valid = if name == "SetThreadAffinityMask" {
                    a[1] == 1
                } else {
                    matches!(a[1], 0 | u32::MAX)
                };
                if !valid {
                    return Err(87);
                }
                Ok((name == "SetThreadAffinityMask") as u32)
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
            "CoInitialize" | "CoInitializeEx" => {
                if a[0] != 0 {
                    return Err(0x80070057);
                }
                let mode = if name == "CoInitialize" { 2 } else { a[1] & 2 };
                if name == "CoInitializeEx" && a[1] & !0xe != 0 {
                    return Err(0x80070057);
                }
                let apartment = state.com_threads.entry(tid).or_insert((mode, 0));
                if apartment.0 != mode {
                    return Err(0x80010106);
                }
                let old = apartment.1;
                apartment.1 = apartment.1.checked_add(1).ok_or(0x80004005u32)?;
                Ok((old != 0) as u32)
            }
            "CoUninitialize" => {
                if let Some((_, count)) = state.com_threads.get_mut(&tid) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        state.com_threads.remove(&tid);
                    }
                }
                Ok(0)
            }
            "SysAllocString" | "SysAllocStringLen" => {
                if name == "SysAllocString" && a[0] == 0 {
                    return Ok(0);
                }
                let text = if name == "SysAllocString" {
                    units(mem, a[0], u32::MAX)?
                } else if a[1] > 1 << 20 {
                    return Err(8);
                } else if a[0] == 0 {
                    vec![0; a[1] as usize]
                } else {
                    units(mem, a[0], a[1])?
                };
                allocate_bstr(mem, state, &text)
            }
            "SysStringLen" | "SysStringByteLen" => {
                if a[0] == 0 {
                    return Ok(0);
                }
                let length = mem
                    .read_u32(a[0].checked_sub(4).ok_or(87u32)?)
                    .map_err(|_| 87u32)?;
                Ok(if name == "SysStringLen" {
                    length / 2
                } else {
                    length
                })
            }
            "SysFreeString" => {
                if a[0] == 0 {
                    return Ok(0);
                }
                let base = state.bstrs.remove(&a[0]).ok_or(87u32)?;
                if !state.heap.free(state.process_heap, base) {
                    return Err(87);
                }
                Ok(0)
            }
            "SysReAllocString" | "SysReAllocStringLen" => {
                let old = mem.read_u32(a[0]).map_err(|_| 87u32)?;
                mem.ptr(a[0], 4, true).map_err(|_| 87u32)?;
                if old != 0 && !state.bstrs.contains_key(&old) {
                    return Err(87);
                }
                if name == "SysReAllocString" && a[1] == 0 {
                    mem.write_u32(a[0], 0).map_err(|_| 87u32)?;
                    if let Some(base) = state.bstrs.remove(&old) {
                        state.heap.free(state.process_heap, base);
                    }
                    return Ok(1);
                }
                let text = if a[1] == 0 || (name.ends_with("Len") && old != 0 && a[1] == old) {
                    if name.ends_with("Len") {
                        if a[2] > 1 << 20 {
                            return Err(8);
                        }
                        let mut text = vec![0; a[2] as usize];
                        // SysReAllocStringLen(NULL source) preserves the old prefix.
                        // Newly grown storage is zeroed by wire's allocator.
                        if old != 0 {
                            let length = mem.read_u32(old - 4).map_err(|_| 87u32)? / 2;
                            let prefix = units(mem, old, length.min(a[2]))?;
                            text[..prefix.len()].copy_from_slice(&prefix);
                        }
                        text
                    } else {
                        Vec::new()
                    }
                } else {
                    units(
                        mem,
                        a[1],
                        if name.ends_with("Len") {
                            a[2]
                        } else {
                            u32::MAX
                        },
                    )?
                };
                let p = allocate_bstr(mem, state, &text)?;
                mem.write_u32(a[0], p).map_err(|_| 87u32)?;
                if let Some(base) = state.bstrs.remove(&old) {
                    state.heap.free(state.process_heap, base);
                }
                Ok(1)
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
