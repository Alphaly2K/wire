//! Process-local registry capabilities differ across guest architectures.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryApi {
    CreateA,
    CreateW,
    CreateExA,
    CreateExW,
    OpenA,
    OpenW,
    OpenExA,
    OpenExW,
    QueryA,
    QueryW,
    QueryExA,
    QueryExW,
    SetExA,
    SetExW,
    DeleteA,
    DeleteW,
    Close,
    EnumKeyA,
    EnumKeyW,
    EnumKeyExA,
    EnumKeyExW,
    EnumValueA,
    EnumValueW,
    QueryInfoA,
    QueryInfoW,
    DeleteKeyA,
    DeleteKeyW,
    DeleteKeyExA,
    DeleteKeyExW,
    DeleteTreeA,
    DeleteTreeW,
    ShellDeleteKeyA,
    ShellDeleteKeyW,
    Flush,
}
impl RegistryApi {
    pub(crate) const fn wide(self) -> bool {
        matches!(
            self,
            Self::CreateW
                | Self::CreateExW
                | Self::OpenW
                | Self::OpenExW
                | Self::QueryW
                | Self::QueryExW
                | Self::SetExW
                | Self::DeleteW
                | Self::EnumKeyW
                | Self::EnumKeyExW
                | Self::EnumValueW
                | Self::QueryInfoW
                | Self::DeleteKeyW
                | Self::DeleteKeyExW
                | Self::DeleteTreeW
                | Self::ShellDeleteKeyW
        )
    }
    pub(crate) const fn dlls(self) -> &'static [&'static str] {
        if matches!(self, Self::ShellDeleteKeyA | Self::ShellDeleteKeyW) {
            &["shlwapi.dll"]
        } else {
            &["advapi32.dll"]
        }
    }
}

const fn declaration(
    id: RegistryApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    use RegistryApi::*;
    let x86 = if matches!(
        id,
        OpenA | OpenW | OpenExA | OpenExW | QueryA | QueryW | QueryExA | QueryExW | Close
    ) {
        ExecutionPolicy::Typed
    } else {
        ExecutionPolicy::LegacyUnimplemented
    };
    let legacy_query = matches!(id, QueryA | QueryW);
    Contract::new(
        ApiId::Registry(id), name, parameters, ReturnConvention::Uint32,
        "PE32 advapi32 open/query report missing guest entries, close accepts original root range, output-first open and LastError preserved; create/set/delete ABI-only, kernel32 private ABI remains Wine fallback; Win64 legacy QueryValueA/W absent, other static advapi32/kernel32/kernelbase imports with dynamic advapi32 or registered gateway lookup; process-local CP932/UTF-16 registry, full-width handles, DWORD options/access/type/length, original permissions, capacity, alias and fault ordering, LSTATUS with unchanged LastError",
        [ArchitecturePlan { dlls: &["advapi32.dll"], execution: x86 },
         ArchitecturePlan {
             dlls: if legacy_query { &[] } else { &["advapi32.dll", "kernel32.dll", "kernelbase.dll"] },
             execution: if legacy_query { ExecutionPolicy::Absent } else { ExecutionPolicy::Typed },
         }],
    ).with_lookup_dlls([&["advapi32.dll"], if legacy_query { &[] } else { &["advapi32.dll"] }])
}

use Parameter::{Pointer as P, Uint32 as D};
const fn added(id: RegistryApi, name: &'static str, parameters: &'static [Parameter]) -> Contract {
    Contract::new(
        ApiId::Registry(id), name, parameters, ReturnConvention::Uint32,
        "Process-local registry enumeration, metadata and key deletion; case-preserving names, access checks, deleted-handle lifetime and atomic outputs; no persistent store, timestamps or security descriptor backend",
        [ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }; 2],
    )
}
pub static REGISTRY_CONTRACTS: [Contract; 34] = [
    declaration(RegistryApi::CreateA, "RegCreateKeyA", &[P, P, P]),
    declaration(RegistryApi::CreateW, "RegCreateKeyW", &[P, P, P]),
    declaration(
        RegistryApi::CreateExA,
        "RegCreateKeyExA",
        &[P, P, D, P, D, D, P, P, P],
    ),
    declaration(
        RegistryApi::CreateExW,
        "RegCreateKeyExW",
        &[P, P, D, P, D, D, P, P, P],
    ),
    declaration(RegistryApi::OpenA, "RegOpenKeyA", &[P, P, P]),
    declaration(RegistryApi::OpenW, "RegOpenKeyW", &[P, P, P]),
    declaration(RegistryApi::OpenExA, "RegOpenKeyExA", &[P, P, D, D, P]),
    declaration(RegistryApi::OpenExW, "RegOpenKeyExW", &[P, P, D, D, P]),
    declaration(RegistryApi::QueryA, "RegQueryValueA", &[P, P, P, P]),
    declaration(RegistryApi::QueryW, "RegQueryValueW", &[P, P, P, P]),
    declaration(
        RegistryApi::QueryExA,
        "RegQueryValueExA",
        &[P, P, P, P, P, P],
    ),
    declaration(
        RegistryApi::QueryExW,
        "RegQueryValueExW",
        &[P, P, P, P, P, P],
    ),
    declaration(RegistryApi::SetExA, "RegSetValueExA", &[P, P, D, D, P, D]),
    declaration(RegistryApi::SetExW, "RegSetValueExW", &[P, P, D, D, P, D]),
    declaration(RegistryApi::DeleteA, "RegDeleteValueA", &[P, P]),
    declaration(RegistryApi::DeleteW, "RegDeleteValueW", &[P, P]),
    declaration(RegistryApi::Close, "RegCloseKey", &[P]),
    added(RegistryApi::EnumKeyA, "RegEnumKeyA", &[P, D, P, D]),
    added(RegistryApi::EnumKeyW, "RegEnumKeyW", &[P, D, P, D]),
    added(
        RegistryApi::EnumKeyExA,
        "RegEnumKeyExA",
        &[P, D, P, P, P, P, P, P],
    ),
    added(
        RegistryApi::EnumKeyExW,
        "RegEnumKeyExW",
        &[P, D, P, P, P, P, P, P],
    ),
    added(
        RegistryApi::EnumValueA,
        "RegEnumValueA",
        &[P, D, P, P, P, P, P, P],
    ),
    added(
        RegistryApi::EnumValueW,
        "RegEnumValueW",
        &[P, D, P, P, P, P, P, P],
    ),
    added(
        RegistryApi::QueryInfoA,
        "RegQueryInfoKeyA",
        &[P, P, P, P, P, P, P, P, P, P, P, P],
    ),
    added(
        RegistryApi::QueryInfoW,
        "RegQueryInfoKeyW",
        &[P, P, P, P, P, P, P, P, P, P, P, P],
    ),
    added(RegistryApi::DeleteKeyA, "RegDeleteKeyA", &[P, P]),
    added(RegistryApi::DeleteKeyW, "RegDeleteKeyW", &[P, P]),
    added(RegistryApi::DeleteKeyExA, "RegDeleteKeyExA", &[P, P, D, D]),
    added(RegistryApi::DeleteKeyExW, "RegDeleteKeyExW", &[P, P, D, D]),
    added(RegistryApi::DeleteTreeA, "RegDeleteTreeA", &[P, P]),
    added(RegistryApi::DeleteTreeW, "RegDeleteTreeW", &[P, P]),
    added(RegistryApi::ShellDeleteKeyA, "SHDeleteKeyA", &[P, P]),
    added(RegistryApi::ShellDeleteKeyW, "SHDeleteKeyW", &[P, P]),
    added(RegistryApi::Flush, "RegFlushKey", &[P]),
];
