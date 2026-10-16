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
    if dll == "winmm.dll" {
        if let Some(contract) = crate::win32::catalog::MULTIMEDIA_CONTRACTS
            .iter()
            .find(|contract| contract.name == name)
        {
            let crate::win32::catalog::ApiId::Multimedia(id) = contract.id else {
                unreachable!();
            };
            return Some(crate::win32::x86::multimedia::call(id, a, mem));
        }
    }
    None
}
