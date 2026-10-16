//! Guest hardware profile and process memory/clock information.
//! Describe the emulated x86 machine, independent of host architecture.
use crate::api::ApiState;
use crate::mem::GuestMem;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn dispatch(
    dll: &str,
    name: &str,
    a: &crate::api::Args,
    mem: &GuestMem,
    state: &mut ApiState,
) -> Option<(u32, String)> {
    // Preserve this public facade's historical exact DLL spelling. Bound API
    // execution calls the typed adapter directly and does not repeat this lookup.
    if dll == "kernel32.dll" {
        if let Some(contract) = crate::win32::catalog::SYSTEM_PROFILE_CONTRACTS
            .iter()
            .find(|contract| contract.name == name)
        {
            let crate::win32::catalog::ApiId::SystemProfile(id) = contract.id else {
                unreachable!();
            };
            return Some(crate::win32::x86::system_profile::call(
                id,
                a[0],
                mem,
                &mut state.last_error,
            ));
        }
    }
    if !(dll == "kernel32.dll" && matches!(name, "GetSystemTime" | "GetLocalTime"))
        && !(dll == "winmm.dll"
            && matches!(name, "timeGetDevCaps" | "timeBeginPeriod" | "timeEndPeriod"))
    {
        return None;
    }
    let result = (|| -> Result<u32, u32> {
        match name {
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
