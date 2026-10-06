//! Guest hardware profile and process memory/clock information.
//! Describe the emulated x86 machine, independent of host architecture.
use crate::api::ApiState;
use crate::mem::GuestMem;
use std::time::{SystemTime, UNIX_EPOCH};
const PHYSICAL: u64 = 512 * 1024 * 1024;
const VIRTUAL: u64 = 0x7fff0000;

pub fn dispatch(
    dll: &str,
    name: &str,
    a: &crate::api::Args,
    mem: &GuestMem,
    state: &mut ApiState,
) -> Option<(u32, String)> {
    if !(dll == "kernel32.dll"
        && matches!(
            name,
            "GetSystemInfo"
                | "GetNativeSystemInfo"
                | "GlobalMemoryStatus"
                | "GlobalMemoryStatusEx"
                | "GetSystemTime"
                | "GetLocalTime"
        ))
        && !(dll == "winmm.dll"
            && matches!(name, "timeGetDevCaps" | "timeBeginPeriod" | "timeEndPeriod"))
    {
        return None;
    }
    let result = (|| -> Result<u32, u32> {
        match name {
            "GetSystemInfo" | "GetNativeSystemInfo" => {
                let mut out = [0u8; 36];
                for (offset, value) in [
                    (0, 0u32),
                    (4, 4096),
                    (8, 0x10000),
                    (12, 0x7ffeffff),
                    (16, 1),
                    (20, 1),
                    (24, 586),
                    (28, 65536),
                ] {
                    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
                }
                out[32..34].copy_from_slice(&6u16.to_le_bytes());
                out[34..36].copy_from_slice(&0x3a09u16.to_le_bytes());
                mem.write_bytes(a[0], &out).map_err(|_| 87u32)?;
                Ok(0)
            }
            "GlobalMemoryStatus" | "GlobalMemoryStatusEx" => {
                let used = mem.application_committed_bytes();
                let available = PHYSICAL.saturating_sub(used);
                let load = ((PHYSICAL - available) * 100 / PHYSICAL) as u32;
                let values = [
                    PHYSICAL,
                    available,
                    PHYSICAL * 2,
                    PHYSICAL * 2 - used.min(PHYSICAL * 2),
                    VIRTUAL,
                    VIRTUAL.saturating_sub(mem.application_reserved_bytes()),
                    0,
                ];
                if name.ends_with("Ex") {
                    if mem.read_u32(a[0]).map_err(|_| 87u32)? != 64 {
                        return Err(87);
                    }
                    let mut out = [0u8; 64];
                    out[..4].copy_from_slice(&64u32.to_le_bytes());
                    out[4..8].copy_from_slice(&load.to_le_bytes());
                    for (i, value) in values.iter().enumerate() {
                        out[8 + i * 8..16 + i * 8].copy_from_slice(&value.to_le_bytes());
                    }
                    mem.write_bytes(a[0], &out).map_err(|_| 87u32)?;
                    Ok(1)
                } else {
                    let mut out = [0u8; 32];
                    out[..4].copy_from_slice(&32u32.to_le_bytes());
                    out[4..8].copy_from_slice(&load.to_le_bytes());
                    for (i, value) in values[..6].iter().enumerate() {
                        out[8 + i * 4..12 + i * 4].copy_from_slice(&(*value as u32).to_le_bytes());
                    }
                    mem.write_bytes(a[0], &out).map_err(|_| 87u32)?;
                    Ok(0)
                }
            }
            "GetSystemTime" | "GetLocalTime" => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| 87u32)?;
                let seconds = now.as_secs() as libc::time_t;
                let mut tm: libc::tm = unsafe { std::mem::zeroed() };
                let result = unsafe {
                    if name == "GetLocalTime" {
                        libc::localtime_r(&seconds, &mut tm)
                    } else {
                        libc::gmtime_r(&seconds, &mut tm)
                    }
                };
                if result.is_null() {
                    return Err(87);
                }
                let fields = [
                    (tm.tm_year + 1900) as u16,
                    (tm.tm_mon + 1) as u16,
                    tm.tm_wday as u16,
                    tm.tm_mday as u16,
                    tm.tm_hour as u16,
                    tm.tm_min as u16,
                    tm.tm_sec as u16,
                    now.subsec_millis() as u16,
                ];
                let bytes: Vec<_> = fields.iter().flat_map(|v| v.to_le_bytes()).collect();
                mem.write_bytes(a[0], &bytes).map_err(|_| 87u32)?;
                Ok(0)
            }
            "timeGetDevCaps" => {
                if a[1] < 8 {
                    return Ok(97);
                }
                let bytes = [1u32.to_le_bytes(), u32::MAX.to_le_bytes()].concat();
                Ok(if mem.write_bytes(a[0], &bytes).is_ok() {
                    0
                } else {
                    97
                })
            }
            "timeBeginPeriod" | "timeEndPeriod" => Ok(if a[0] == 0 { 97 } else { 0 }),
            _ => unreachable!(),
        }
    })();
    Some(match result {
        Ok(v) => (v, "x86 guest hardware / monotonic clock profile".into()),
        Err(e) => {
            state.last_error = e;
            (0, format!("Win32 error {e}"))
        }
    })
}
