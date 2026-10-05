//! Synchronous Win32 file operations and directory queries.
use crate::api::{ApiState, HandleObj};
use crate::fs::win_error;
use crate::mem::GuestMem;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub fn read_name(mem: &GuestMem, ptr: u32, wide: bool) -> Result<String, u32> {
    if wide {
        return mem.read_wstr(ptr, 32768).map_err(|_| 87);
    }
    let mut bytes = Vec::new();
    for i in 0..32768 {
        let b = mem.read_u8(ptr.wrapping_add(i)).map_err(|_| 87u32)?;
        if b == 0 {
            return crate::codepage::decode(&bytes, 932);
        }
        bytes.push(b);
    }
    Err(123)
}

pub fn write_string(mem: &GuestMem, ptr: u32, cap: u32, s: &str, wide: bool) -> Result<u32, u32> {
    let mut bytes = if wide {
        s.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    } else {
        crate::codepage::encode(s, 932)?.0
    };
    let width = if wide { 2 } else { 1 };
    let len = bytes.len() as u32 / width;
    if cap <= len {
        return Ok(len + 1);
    }
    bytes.extend(std::iter::repeat_n(0, width as usize));
    mem.write_bytes(ptr, &bytes).map_err(|_| 87u32)?;
    Ok(len)
}

fn attributes(path: &Path) -> Result<u32, u32> {
    let m = path.metadata().map_err(|e| win_error(&e))?;
    Ok((if m.is_dir() { 0x10 } else { 0x80 }) | m.permissions().readonly() as u32)
}

fn find_data(mem: &GuestMem, ptr: u32, path: &Path, wide: bool) -> Result<(), u32> {
    let m = path.metadata().map_err(|e| win_error(&e))?;
    mem.write_bytes(ptr, &vec![0; if wide { 592 } else { 320 }])
        .map_err(|_| 87u32)?;
    mem.write_u32(ptr, attributes(path)?).map_err(|_| 87u32)?;
    mem.write_u32(ptr + 28, (m.len() >> 32) as u32)
        .map_err(|_| 87u32)?;
    mem.write_u32(ptr + 32, m.len() as u32).map_err(|_| 87u32)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    write_string(mem, ptr + 44, 260, &name, wide)?;
    Ok(())
}

fn wildcard(pattern: &str, name: &str) -> bool {
    let pattern = if pattern == "*.*" { "*" } else { pattern };
    let p: Vec<_> = pattern.to_lowercase().chars().collect();
    let n: Vec<_> = name.to_lowercase().chars().collect();
    let mut dp = vec![false; n.len() + 1];
    dp[0] = true;
    for c in p {
        let mut next = vec![false; n.len() + 1];
        if c == '*' {
            next[0] = dp[0];
        }
        for i in 1..=n.len() {
            next[i] = if c == '*' {
                dp[i] || next[i - 1]
            } else {
                dp[i - 1] && (c == '?' || c == n[i - 1])
            };
        }
        dp = next;
    }
    dp[n.len()]
}

pub fn dispatch(
    dll: &str,
    name: &str,
    a: &crate::api::Args,
    mem: &GuestMem,
    state: &mut ApiState,
) -> Option<(u32, String)> {
    let supported = dll == "kernel32.dll"
        && matches!(
            name,
            "CreateFileA"
                | "CreateFileW"
                | "ReadFile"
                | "WriteFile"
                | "GetFileSize"
                | "GetFileSizeEx"
                | "SetFilePointer"
                | "SetFilePointerEx"
                | "SetEndOfFile"
                | "FlushFileBuffers"
                | "GetCurrentDirectoryA"
                | "GetCurrentDirectoryW"
                | "SetCurrentDirectoryA"
                | "SetCurrentDirectoryW"
                | "GetFullPathNameA"
                | "GetFullPathNameW"
                | "GetFileAttributesA"
                | "GetFileAttributesW"
                | "CreateDirectoryA"
                | "CreateDirectoryW"
                | "FindFirstFileA"
                | "FindFirstFileW"
                | "FindFirstFileExA"
                | "FindFirstFileExW"
                | "FindNextFileA"
                | "FindNextFileW"
                | "FindClose"
        )
        || dll == "shlwapi.dll"
            && matches!(
                name,
                "PathFileExistsA" | "PathFileExistsW" | "PathIsDirectoryA" | "PathIsDirectoryW"
            );
    if !supported {
        return None;
    }
    let wide = name.ends_with('W');
    let failure = if matches!(
        name,
        "CreateFileA"
            | "CreateFileW"
            | "GetFileSize"
            | "SetFilePointer"
            | "GetFileAttributesA"
            | "GetFileAttributesW"
            | "FindFirstFileA"
            | "FindFirstFileW"
            | "FindFirstFileExA"
            | "FindFirstFileExW"
    ) {
        u32::MAX
    } else {
        0
    };
    let result = (|| -> Result<(u32, String), u32> {
        match name {
            "CreateFileA" | "CreateFileW" => {
                if a[6] != 0 || a[5] & 0x4000_0000 != 0 {
                    return Err(50);
                } // OVERLAPPED not implemented
                let text = read_name(mem, a[0], wide)?;
                let path = state.fs.as_ref().ok_or(3u32)?.resolve(&text)?;
                let exists = path.exists();
                let readable = a[1] & 0x8000_0000 != 0 || a[1] == 0;
                let writable = a[1] & 0x4000_0000 != 0;
                let mut opts = std::fs::OpenOptions::new();
                opts.read(readable).write(writable);
                match a[4] {
                    1 => {
                        opts.create_new(true);
                    }
                    2 => {
                        opts.create(true).truncate(true);
                    }
                    3 => {}
                    4 => {
                        opts.create(true);
                    }
                    5 => {
                        opts.truncate(true);
                    }
                    _ => return Err(87),
                }
                let file = opts.open(&path).map_err(|e| win_error(&e))?;
                state.last_error = if exists && matches!(a[4], 2 | 4) {
                    183
                } else {
                    0
                };
                let h = state.alloc_handle(HandleObj::File {
                    file,
                    readable,
                    writable,
                });
                Ok((h, format!("file {text:?}")))
            }
            "ReadFile" | "WriteFile" => {
                if a[4] != 0 {
                    return Err(50);
                }
                if a[3] == 0 {
                    return Err(87);
                }
                mem.write_u32(a[3], 0).map_err(|_| 87u32)?;
                let Some(HandleObj::File {
                    file,
                    readable,
                    writable,
                }) = state.handles.get_mut(&a[0])
                else {
                    return Err(6);
                };
                let reading = name == "ReadFile";
                if reading && !*readable || !reading && !*writable {
                    return Err(5);
                }
                let ptr = mem.ptr(a[1], a[2], reading).map_err(|_| 87u32)?;
                let n = if reading {
                    // ptr() validated the full buffer before creating the slice.
                    file.read(unsafe { std::slice::from_raw_parts_mut(ptr, a[2] as usize) })
                } else {
                    file.write(unsafe { std::slice::from_raw_parts(ptr, a[2] as usize) })
                }
                .map_err(|e| win_error(&e))?;
                mem.write_u32(a[3], n as u32).map_err(|_| 87u32)?;
                Ok((1, format!("{n} bytes")))
            }
            "GetFileSize" | "GetFileSizeEx" => {
                let Some(HandleObj::File { file, .. }) = state.handles.get(&a[0]) else {
                    return Err(6);
                };
                let n = file.metadata().map_err(|e| win_error(&e))?.len();
                if name == "GetFileSizeEx" {
                    mem.write_u64(a[1], n).map_err(|_| 87u32)?;
                    Ok((1, String::new()))
                } else {
                    if a[1] != 0 {
                        mem.write_u32(a[1], (n >> 32) as u32).map_err(|_| 87u32)?;
                    }
                    state.last_error = 0;
                    Ok((n as u32, String::new()))
                }
            }
            "SetFilePointer" | "SetFilePointerEx" => {
                let ex = name.ends_with("Ex");
                let distance = if ex {
                    ((a[2] as u64) << 32 | a[1] as u64) as i64
                } else if a[2] != 0 {
                    ((mem.read_u32(a[2]).map_err(|_| 87u32)? as u64) << 32 | a[1] as u64) as i64
                } else {
                    a[1] as i32 as i64
                };
                let origin = if ex { a[4] } else { a[3] };
                let seek = match origin {
                    0 if distance >= 0 => SeekFrom::Start(distance as u64),
                    0 => return Err(131),
                    1 => SeekFrom::Current(distance),
                    2 => SeekFrom::End(distance),
                    _ => return Err(87),
                };
                let Some(HandleObj::File { file, .. }) = state.handles.get_mut(&a[0]) else {
                    return Err(6);
                };
                let pos = file.seek(seek).map_err(|e| win_error(&e))?;
                state.last_error = 0;
                if ex {
                    if a[3] != 0 {
                        mem.write_u64(a[3], pos).map_err(|_| 87u32)?;
                    }
                    Ok((1, String::new()))
                } else {
                    if a[2] != 0 {
                        mem.write_u32(a[2], (pos >> 32) as u32).map_err(|_| 87u32)?;
                    }
                    Ok((pos as u32, String::new()))
                }
            }
            "SetEndOfFile" | "FlushFileBuffers" => {
                let Some(HandleObj::File { file, writable, .. }) = state.handles.get_mut(&a[0])
                else {
                    return Err(6);
                };
                if !*writable {
                    return Err(5);
                }
                if name == "SetEndOfFile" {
                    let n = file.stream_position().map_err(|e| win_error(&e))?;
                    file.set_len(n).map_err(|e| win_error(&e))?;
                } else {
                    file.sync_all().map_err(|e| win_error(&e))?;
                }
                Ok((1, String::new()))
            }
            "GetCurrentDirectoryA" | "GetCurrentDirectoryW" => {
                let cwd = state.fs.as_ref().ok_or(3u32)?.cwd();
                Ok((write_string(mem, a[1], a[0], &cwd, wide)?, cwd))
            }
            "SetCurrentDirectoryA" | "SetCurrentDirectoryW" => {
                let text = read_name(mem, a[0], wide)?;
                state.fs.as_mut().ok_or(3u32)?.set_cwd(&text)?;
                Ok((1, text))
            }
            "GetFullPathNameA" | "GetFullPathNameW" => {
                let text = read_name(mem, a[0], wide)?;
                let fs = state.fs.as_ref().ok_or(3u32)?;
                let path = fs.resolve(&text)?;
                let full = fs.guest_path(&path);
                let ret = write_string(mem, a[2], a[1], &full, wide)?;
                if a[3] != 0 && ret < a[1] {
                    let start = full.rfind('\\').map(|i| i + 1).unwrap_or(0);
                    let offset = if wide {
                        full[..start].encode_utf16().count() as u32 * 2
                    } else {
                        crate::codepage::encode(&full[..start], 932)?.0.len() as u32
                    };
                    mem.write_u32(a[3], a[2] + offset).map_err(|_| 87u32)?;
                }
                Ok((ret, full))
            }
            "GetFileAttributesA" | "GetFileAttributesW" | "PathFileExistsA" | "PathFileExistsW"
            | "PathIsDirectoryA" | "PathIsDirectoryW" => {
                let text = read_name(mem, a[0], wide)?;
                let path = state.fs.as_ref().ok_or(3u32)?.resolve(&text)?;
                let attr = attributes(&path)?;
                Ok((
                    if name.starts_with("PathIsDirectory") {
                        (attr & 0x10 != 0) as u32
                    } else if name.starts_with("Path") {
                        1
                    } else {
                        attr
                    },
                    text,
                ))
            }
            "CreateDirectoryA" | "CreateDirectoryW" => {
                let text = read_name(mem, a[0], wide)?;
                let path = state.fs.as_ref().ok_or(3u32)?.resolve(&text)?;
                std::fs::create_dir(path).map_err(|e| win_error(&e))?;
                Ok((1, text))
            }
            "FindFirstFileA" | "FindFirstFileW" | "FindFirstFileExA" | "FindFirstFileExW" => {
                let extended = name.starts_with("FindFirstFileEx");
                if extended && (a[1] > 1 || a[3] != 0 || a[4] != 0 || a[5] & !2 != 0) {
                    return Err(87);
                }
                let output = if extended { a[2] } else { a[1] };
                let text = read_name(mem, a[0], wide)?.replace('\\', "/");
                let (dir, pattern) = text.rsplit_once('/').unwrap_or((".", text.as_str()));
                let fs = state.fs.as_ref().ok_or(3u32)?;
                let parent = fs.resolve(if dir == "C:" {
                    "C:/"
                } else if dir.is_empty() {
                    "/"
                } else {
                    dir
                })?;
                let mut entries = Vec::new();
                for e in std::fs::read_dir(parent)
                    .map_err(|e| win_error(&e))?
                    .flatten()
                {
                    if wildcard(pattern, &e.file_name().to_string_lossy())
                        && fs.resolve(&fs.guest_path(&e.path())).is_ok()
                    {
                        entries.push(e.path());
                    }
                }
                entries.sort_by_key(|p| p.to_string_lossy().to_lowercase());
                let first = entries.first().ok_or(2u32)?;
                find_data(mem, output, first, wide)?;
                Ok((
                    state.alloc_handle(HandleObj::Search { entries, next: 1 }),
                    text,
                ))
            }
            "FindNextFileA" | "FindNextFileW" => {
                let Some(HandleObj::Search { entries, next }) = state.handles.get_mut(&a[0]) else {
                    return Err(6);
                };
                let p = entries.get(*next).ok_or(18u32)?;
                find_data(mem, a[1], p, wide)?;
                *next += 1;
                Ok((1, String::new()))
            }
            "FindClose" => {
                if !matches!(state.handles.get(&a[0]), Some(HandleObj::Search { .. })) {
                    return Err(6);
                }
                state.handles.remove(&a[0]);
                Ok((1, String::new()))
            }
            _ => unreachable!(),
        }
    })();
    Some(match result {
        Ok(v) => v,
        Err(e) => {
            state.last_error = e;
            let path = if matches!(name, "CreateFileA" | "CreateFileW") {
                read_name(mem, a[0], wide)
                    .map(|path| format!("; file {path:?}"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            (failure, format!("Win32 error {e}{path}"))
        }
    })
}
