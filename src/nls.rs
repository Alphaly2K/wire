//! Compatibility facade; NLS, timezone, task and shell calls use their domain adapters.
use crate::api::ApiState;
use crate::mem::GuestMem;

pub fn dispatch(
    dll: &str,
    name: &str,
    a: &crate::api::Args,
    mem: &mut GuestMem,
    state: &mut ApiState,
) -> Option<(u32, String)> {
    // Compatibility entry point keeps its original scope; the main gateway
    // binds these calls directly, so this lookup is outside the typed hot path.
    if dll == "kernel32.dll"
        && matches!(
            name,
            "MultiByteToWideChar"
                | "WideCharToMultiByte"
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
        )
    {
        use crate::win32::catalog::{resolve, ApiId, Architecture, Resolution};
        let Resolution::Bound(contract) = resolve(Architecture::X86, dll, name) else {
            unreachable!("legacy NLS facade has no contract");
        };
        let ApiId::Nls(id) = contract.id else {
            unreachable!();
        };
        return Some(
            match crate::win32::x86::nls::call(
                id,
                a,
                mem,
                &state.environment,
                &mut state.last_error,
            ) {
                Ok(value) => (value.value, value.note),
                Err(_) => {
                    state.last_error = 87;
                    (0, "Win32 error 87".into())
                }
            },
        );
    }
    if dll == "kernel32.dll" && name == "GetTimeZoneInformation" {
        return Some(crate::win32::x86::calendar::call(
            crate::win32::catalog::CalendarApi::TimeZone,
            a[0],
            mem,
            &mut state.last_error,
        ));
    }
    // Public legacy facade: task memory moved to its COM adapter, while this
    // entry retains its original exact provider and export scope.
    if dll == "ole32.dll" {
        use crate::win32::catalog::{resolve, ApiId, Architecture, ComApi, Resolution};
        if let Resolution::Bound(contract) = resolve(Architecture::X86, dll, name) {
            if let ApiId::Com(id @ (ComApi::TaskAlloc | ComApi::TaskFree)) = contract.id {
                return Some(crate::win32::x86::com::task_call(
                    id,
                    a,
                    mem,
                    &mut state.heap,
                    state.process_heap,
                    &mut state.last_error,
                ));
            }
        }
    }
    if !(dll == "shell32.dll"
        && matches!(
            name,
            "SHGetSpecialFolderPathA"
                | "SHGetSpecialFolderPathW"
                | "SHGetFolderPathA"
                | "SHGetFolderPathW"
                | "SHGetKnownFolderPath"
        ))
    {
        return None;
    }
    use crate::win32::catalog::{resolve, ApiId, Architecture, Resolution};
    let Resolution::Bound(contract) = resolve(Architecture::X86, dll, name) else {
        unreachable!("legacy shell facade has no contract");
    };
    let ApiId::Shell(id) = contract.id else {
        unreachable!()
    };
    Some(crate::win32::x86::shell::call(
        id,
        a,
        mem,
        &mut state.heap,
        state.process_heap,
        &mut state.last_error,
    ))
}
