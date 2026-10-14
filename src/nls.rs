//! Legacy shell allocation and timezone facade; NLS has typed architecture adapters.
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
    if !((dll == "kernel32.dll" && matches!(name, "GetTimeZoneInformation"))
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
