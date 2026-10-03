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
    a: &[u32; 12],
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
            | "GetSystemDefaultLangID" => Ok(0x411),
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
