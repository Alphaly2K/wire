//! Guest hardware profile and process memory/clock information.
//! Describe the emulated x86 machine, independent of host architecture.
use crate::api::ApiState;
use crate::mem::GuestMem;

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
        // Keep this facade limited to its original two calendar exports.
        if matches!(name, "GetSystemTime" | "GetLocalTime") {
            use crate::win32::catalog::{resolve, ApiId, Architecture, Resolution};
            let Resolution::Bound(contract) = resolve(Architecture::X86, dll, name) else {
                unreachable!("legacy system calendar facade has no contract");
            };
            let ApiId::Calendar(id) = contract.id else {
                unreachable!();
            };
            return Some(crate::win32::x86::calendar::call(
                id,
                a[0],
                mem,
                &mut state.last_error,
            ));
        }
    }
    if !(dll == "winmm.dll"
        && matches!(name, "timeGetDevCaps" | "timeBeginPeriod" | "timeEndPeriod"))
    {
        return None;
    }
    let result = (|| -> Result<u32, u32> {
        match name {
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
