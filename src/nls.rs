//! Minimal code-page conversion and sandboxed shell-directory APIs.
use crate::api::ApiState;
use crate::mem::GuestMem;

fn input(mem: &GuestMem, ptr: u32, len: u32, width: u32) -> Result<Vec<u8>, u32> {
    if ptr == 0 || len == 0 || (len as i32) < -1 {
        return Err(87);
    }
    let n = if len == u32::MAX {
        let mut n = 0u32;
        loop {
            let value = if width == 2 {
                mem.read_u16(ptr.checked_add(n * 2).ok_or(87u32)?)
                    .map_err(|_| 87u32)? as u32
            } else {
                mem.read_u8(ptr.checked_add(n).ok_or(87u32)?)
                    .map_err(|_| 87u32)? as u32
            };
            n += 1;
            if value == 0 {
                break n;
            }
            if n > 1 << 24 {
                return Err(87);
            }
        }
    } else {
        len
    };
    let size = n.checked_mul(width).ok_or(87u32)?;
    mem.ptr(ptr, size, false).map_err(|_| 87u32)?;
    let mut bytes = vec![0; size as usize];
    mem.read_bytes(ptr, &mut bytes).map_err(|_| 87u32)?;
    Ok(bytes)
}

pub fn dispatch(
    dll: &str,
    name: &str,
    a: &crate::api::Args,
    mem: &mut GuestMem,
    state: &mut ApiState,
) -> Option<(u32, String)> {
    if !((dll == "kernel32.dll"
        && matches!(
            name,
            "MultiByteToWideChar"
                | "WideCharToMultiByte"
                | "GetTimeZoneInformation"
                | "GetUserDefaultLCID"
                | "GetSystemDefaultLCID"
                | "GetThreadLocale"
                | "GetUserDefaultLangID"
                | "GetSystemDefaultLangID"
                | "IsValidCodePage"
                | "GetStringTypeW"
                | "GetStringTypeExW"
                | "LCMapStringW"
                | "LCMapStringA"
                | "LCMapStringEx"
        ))
        || (dll == "shell32.dll"
            && matches!(
                name,
                "SHGetSpecialFolderPathA"
                    | "SHGetSpecialFolderPathW"
                    | "SHGetFolderPathA"
                    | "SHGetFolderPathW"
                    | "SHGetKnownFolderPath"
            ))
        || (dll == "ole32.dll" && matches!(name, "CoTaskMemAlloc" | "CoTaskMemFree")))
    {
        return None;
    }
    let result = (|| -> Result<u32, u32> {
        match name {
            "GetStringTypeW" | "GetStringTypeExW" => {
                let shift = usize::from(name == "GetStringTypeExW");
                if a[shift] != 1 {
                    return Err(1004);
                } // CT_CTYPE1
                let bytes = input(mem, a[shift + 1], a[shift + 2], 2)?;
                let output: Vec<_> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .flat_map(|b| ctype1(u16::from_le_bytes(*b)).to_le_bytes())
                    .collect();
                mem.write_bytes(a[shift + 3], &output).map_err(|_| 87u32)?;
                Ok(1)
            }
            "LCMapStringW" | "LCMapStringA" | "LCMapStringEx" => {
                let upper = a[1] & 0x200 != 0;
                let lower = a[1] & 0x100 != 0;
                if upper == lower || a[1] & !0x0100_0300 != 0 {
                    return Err(1004);
                }
                let wide = name != "LCMapStringA";
                let bytes = input(mem, a[2], a[3], if wide { 2 } else { 1 })?;
                let text = if wide {
                    let units: Vec<_> = bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes(*b))
                        .collect();
                    String::from_utf16(&units).map_err(|_| 1113u32)?
                } else {
                    crate::codepage::decode(&bytes, 932)?
                };
                let mapped = if upper {
                    text.to_uppercase()
                } else {
                    text.to_lowercase()
                };
                let output = if wide {
                    mapped.encode_utf16().flat_map(u16::to_le_bytes).collect()
                } else {
                    crate::codepage::encode(&mapped, 932)?.0
                };
                let count = output.len() as u32 / if wide { 2 } else { 1 };
                if a[5] == 0 {
                    return Ok(count);
                }
                if a[5] < count {
                    return Err(122);
                }
                mem.write_bytes(a[4], &output).map_err(|_| 87u32)?;
                Ok(count)
            }
            "MultiByteToWideChar" => {
                let bytes = input(mem, a[2], a[3], 1)?;
                let s = if a[1] & 8 != 0 {
                    crate::codepage::decode(&bytes, a[0])?
                } else {
                    crate::codepage::decode_lossy(&bytes, a[0])?
                };
                let out: Vec<_> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
                let n = out.len() as u32 / 2;
                if a[5] == 0 {
                    return Ok(n);
                }
                if a[5] < n {
                    return Err(122);
                }
                mem.write_bytes(a[4], &out).map_err(|_| 87u32)?;
                Ok(n)
            }
            "WideCharToMultiByte" => {
                let bytes = input(mem, a[2], a[3], 2)?;
                let wide: Vec<_> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect();
                let s = String::from_utf16(&wide).map_err(|_| 1113u32)?;
                let (out, used) = crate::codepage::encode(&s, a[0])?;
                if a[7] != 0 {
                    mem.write_u32(a[7], used as u32).map_err(|_| 87u32)?;
                }
                let n = out.len() as u32;
                if a[5] == 0 {
                    return Ok(n);
                }
                if a[5] < n {
                    return Err(122);
                }
                mem.write_bytes(a[4], &out).map_err(|_| 87u32)?;
                Ok(n)
            }
            "GetTimeZoneInformation" => {
                mem.write_bytes(a[0], &[0; 172]).map_err(|_| 87u32)?;
                Ok(0)
            }
            "IsValidCodePage" => Ok(matches!(a[0], 932 | 1252 | 65001) as u32),
            "GetUserDefaultLCID"
            | "GetSystemDefaultLCID"
            | "GetThreadLocale"
            | "GetUserDefaultLangID"
            | "GetSystemDefaultLangID" => Ok(state.environment.locale_id()),
            "SHGetSpecialFolderPathA" | "SHGetSpecialFolderPathW" => {
                crate::file_api::write_string(mem, a[1], 260, "C:\\", name.ends_with('W'))?;
                Ok(1)
            }
            "SHGetFolderPathA" | "SHGetFolderPathW" => {
                crate::file_api::write_string(mem, a[4], 260, "C:\\", name.ends_with('W'))?;
                Ok(0)
            }
            "SHGetKnownFolderPath" => {
                mem.write_u32(a[3], 0).map_err(|_| 87u32)?;
                let p = state
                    .heap
                    .alloc(mem, state.process_heap, 8, false)
                    .map_err(|_| 8u32)?;
                if p == 0 {
                    return Err(8);
                }
                crate::file_api::write_string(mem, p, 4, "C:\\", true)?;
                mem.write_u32(a[3], p).map_err(|_| 87u32)?;
                Ok(0)
            }
            "CoTaskMemAlloc" => state
                .heap
                .alloc(mem, state.process_heap, a[0], false)
                .map_err(|_| 8u32),
            "CoTaskMemFree" => {
                state.heap.free(state.process_heap, a[0]);
                Ok(0)
            }
            _ => unreachable!(),
        }
    })();
    Some(match result {
        Ok(v) => (v, "code-page / sandbox directory shim".into()),
        Err(e) => {
            state.last_error = e;
            (
                if name == "SHGetKnownFolderPath" || name.starts_with("SHGetFolderPath") {
                    0x8007_0000 | e
                } else {
                    0
                },
                format!("Win32 error {e}"),
            )
        }
    })
}

/// C1 flags needed by the CRT's character tables. Unicode properties follow
/// Rust's Unicode tables; this is not a complete Windows NLS database.
pub(crate) fn ctype1(unit: u16) -> u16 {
    let Some(c) = char::from_u32(unit as u32) else {
        return 0;
    };
    if (0xfdd0..=0xfdef).contains(&unit) || unit >= 0xfffe {
        return 0;
    }
    let mut flags = 0x200;
    if c.is_uppercase() {
        flags |= 1;
    }
    if c.is_lowercase() {
        flags |= 2;
    }
    if c.is_ascii_digit() || ('０'..='９').contains(&c) {
        flags |= 4;
    }
    if c.is_whitespace() {
        flags |= 8;
    }
    if c.is_control() {
        flags |= 0x20;
    }
    if matches!(
        c,
        '\t' | ' ' | '\u{a0}' | '\u{1680}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    ) || ('\u{2000}'..='\u{200a}').contains(&c)
    {
        flags |= 0x40;
    }
    if c.is_ascii_hexdigit()
        || ('Ａ'..='Ｆ').contains(&c)
        || ('ａ'..='ｆ').contains(&c)
        || ('０'..='９').contains(&c)
    {
        flags |= 0x80;
    }
    if c.is_alphabetic() {
        flags |= 0x100;
    }
    if !c.is_alphanumeric() && !c.is_whitespace() && !c.is_control() {
        flags |= 0x10;
    }
    flags
}
