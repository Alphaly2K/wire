use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleApi {
    GetModuleHandleA,
    GetModuleHandleW,
    GetModuleHandleExA,
    GetModuleHandleExW,
    GetModuleFileNameA,
    GetModuleFileNameW,
    LoadLibraryA,
    LoadLibraryW,
    LoadLibraryExA,
    LoadLibraryExW,
    GetProcAddress,
    FreeLibrary,
    DisableThreadLibraryCalls,
    GetDllDirectoryA,
    GetDllDirectoryW,
    SetDllDirectoryA,
    SetDllDirectoryW,
}

impl ModuleApi {
    pub(crate) fn wide(self) -> bool {
        matches!(
            self,
            Self::GetModuleHandleW
                | Self::GetModuleHandleExW
                | Self::GetModuleFileNameW
                | Self::LoadLibraryW
                | Self::LoadLibraryExW
                | Self::GetDllDirectoryW
                | Self::SetDllDirectoryW
        )
    }
    pub(crate) fn extended(self) -> bool {
        matches!(
            self,
            Self::GetModuleHandleExA
                | Self::GetModuleHandleExW
                | Self::LoadLibraryExA
                | Self::LoadLibraryExW
        )
    }
    pub(crate) fn control(self) -> bool {
        matches!(
            self,
            Self::LoadLibraryA
                | Self::LoadLibraryW
                | Self::LoadLibraryExA
                | Self::LoadLibraryExW
                | Self::GetProcAddress
                | Self::FreeLibrary
        )
    }
}

const fn declaration(
    id: ModuleApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Module(id),
        name,
        parameters,
        returns,
        "existing guest/provider identities and confined loader; PE32 DLL/TLS callbacks and rollback; Win64 load requests and guest detach refusal; architecture-specific output and return-frame order",
        [
            ArchitecturePlan {
                dlls: &["kernel32.dll"],
                execution: ExecutionPolicy::Typed,
            },
            ArchitecturePlan {
                dlls: &["kernel32.dll", "kernelbase.dll"],
                execution: ExecutionPolicy::Typed,
            },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as R, Uint32 as U};

pub static MODULE_CONTRACTS: [Contract; 17] = [
    declaration(ModuleApi::GetModuleHandleA, "GetModuleHandleA", &[P], R),
    declaration(ModuleApi::GetModuleHandleW, "GetModuleHandleW", &[P], R),
    declaration(
        ModuleApi::GetModuleHandleExA,
        "GetModuleHandleExA",
        &[D, P, P],
        B,
    ),
    declaration(
        ModuleApi::GetModuleHandleExW,
        "GetModuleHandleExW",
        &[D, P, P],
        B,
    ),
    declaration(
        ModuleApi::GetModuleFileNameA,
        "GetModuleFileNameA",
        &[P, P, D],
        U,
    ),
    declaration(
        ModuleApi::GetModuleFileNameW,
        "GetModuleFileNameW",
        &[P, P, D],
        U,
    ),
    declaration(ModuleApi::LoadLibraryA, "LoadLibraryA", &[P], R),
    declaration(ModuleApi::LoadLibraryW, "LoadLibraryW", &[P], R),
    declaration(ModuleApi::LoadLibraryExA, "LoadLibraryExA", &[P, P, D], R),
    declaration(ModuleApi::LoadLibraryExW, "LoadLibraryExW", &[P, P, D], R),
    declaration(ModuleApi::GetProcAddress, "GetProcAddress", &[P, P], R),
    declaration(ModuleApi::FreeLibrary, "FreeLibrary", &[P], B),
    declaration(
        ModuleApi::DisableThreadLibraryCalls,
        "DisableThreadLibraryCalls",
        &[P],
        B,
    ),
    directory(ModuleApi::GetDllDirectoryA, "GetDllDirectoryA", &[D, P], U),
    directory(ModuleApi::GetDllDirectoryW, "GetDllDirectoryW", &[D, P], U),
    directory(ModuleApi::SetDllDirectoryA, "SetDllDirectoryA", &[P], B),
    directory(ModuleApi::SetDllDirectoryW, "SetDllDirectoryW", &[P], B),
];

const fn directory(
    id: ModuleApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let mut contract = declaration(id, name, parameters, returns);
    contract.plans[0].execution = ExecutionPolicy::LegacyUnimplemented;
    contract
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleQueryApi {
    EnumProcessModules,
    EnumProcessModulesEx,
    GetModuleInformation,
    GetModuleBaseNameA,
    GetModuleBaseNameW,
    GetModuleFileNameExA,
    GetModuleFileNameExW,
}
impl ModuleQueryApi {
    pub(crate) fn wide(self) -> bool {
        matches!(self, Self::GetModuleBaseNameW | Self::GetModuleFileNameExW)
    }
}

const fn query(id: ModuleQueryApi, name: &'static str, kernel_alias: bool) -> Contract {
    let parameters: &'static [Parameter] = match id {
        ModuleQueryApi::EnumProcessModules => &[P, P, D, P],
        ModuleQueryApi::EnumProcessModulesEx => &[P, P, D, P, D],
        _ => &[P, P, P, D],
    };
    let returns = match id {
        ModuleQueryApi::EnumProcessModules
        | ModuleQueryApi::EnumProcessModulesEx
        | ModuleQueryApi::GetModuleInformation => B,
        _ => U,
    };
    Contract::new(
        ApiId::ModuleQuery(id), name, parameters, returns,
        "current-process modules; historical Win64 kernel/psapi static aliases; fresh-provider lookup exposes only K32 kernel aliases, with the existing bound-gateway fallback preserved; PE32 K32 remains Wine ABI-only, unprefixed queries remain absent",
        [
            ArchitecturePlan {
                dlls: if kernel_alias { &["kernel32.dll"] } else { &[] },
                execution: if kernel_alias { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Absent },
            },
            ArchitecturePlan {
                dlls: &["kernel32.dll", "kernelbase.dll", "psapi.dll"],
                execution: ExecutionPolicy::Typed,
            },
        ],
    ).with_lookup_dlls([
        if kernel_alias { &["kernel32.dll"] } else { &[] },
        if kernel_alias { &["kernel32.dll", "kernelbase.dll"] } else { &[] },
    ])
}

pub static MODULE_QUERY_CONTRACTS: [Contract; 14] = [
    query(
        ModuleQueryApi::EnumProcessModules,
        "EnumProcessModules",
        false,
    ),
    query(
        ModuleQueryApi::EnumProcessModules,
        "K32EnumProcessModules",
        true,
    ),
    query(
        ModuleQueryApi::EnumProcessModulesEx,
        "EnumProcessModulesEx",
        false,
    ),
    query(
        ModuleQueryApi::EnumProcessModulesEx,
        "K32EnumProcessModulesEx",
        true,
    ),
    query(
        ModuleQueryApi::GetModuleInformation,
        "GetModuleInformation",
        false,
    ),
    query(
        ModuleQueryApi::GetModuleInformation,
        "K32GetModuleInformation",
        true,
    ),
    query(
        ModuleQueryApi::GetModuleBaseNameA,
        "GetModuleBaseNameA",
        false,
    ),
    query(
        ModuleQueryApi::GetModuleBaseNameA,
        "K32GetModuleBaseNameA",
        true,
    ),
    query(
        ModuleQueryApi::GetModuleBaseNameW,
        "GetModuleBaseNameW",
        false,
    ),
    query(
        ModuleQueryApi::GetModuleBaseNameW,
        "K32GetModuleBaseNameW",
        true,
    ),
    query(
        ModuleQueryApi::GetModuleFileNameExA,
        "GetModuleFileNameExA",
        false,
    ),
    query(
        ModuleQueryApi::GetModuleFileNameExA,
        "K32GetModuleFileNameExA",
        true,
    ),
    query(
        ModuleQueryApi::GetModuleFileNameExW,
        "GetModuleFileNameExW",
        false,
    ),
    query(
        ModuleQueryApi::GetModuleFileNameExW,
        "K32GetModuleFileNameExW",
        true,
    ),
];
