use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalMemoryApi {
    Alloc,
    ReAlloc,
    Free,
    Size,
    Lock,
    Unlock,
    Flags,
}

const fn declaration(
    id: LocalMemoryApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
    pe32_typed: bool,
) -> Contract {
    Contract::new(
        ApiId::LocalMemory(id), name, parameters, returns,
        "existing identical Local/Global bodies and separate architecture allocation owners; PE32 process heap sharing, identity lock and preserved LastError, ReAlloc/Flags ABI-only; Win64 local block membership, fixed/move/zero flags subset, DWORD flags and pointer-sized allocation/returns, checked free/realloc fault and saved return order",
        [
            ArchitecturePlan {
                dlls: &["kernel32.dll"],
                execution: if pe32_typed { ExecutionPolicy::Typed } else { ExecutionPolicy::LegacyUnimplemented },
            },
            ArchitecturePlan {
                dlls: &["kernel32.dll", "kernelbase.dll"],
                execution: ExecutionPolicy::Typed,
            },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as H, Uint32 as U};

pub static LOCAL_MEMORY_CONTRACTS: [Contract; 14] = [
    declaration(LocalMemoryApi::Alloc, "LocalAlloc", &[D, P], H, true),
    declaration(LocalMemoryApi::Alloc, "GlobalAlloc", &[D, P], H, true),
    declaration(
        LocalMemoryApi::ReAlloc,
        "LocalReAlloc",
        &[P, P, D],
        H,
        false,
    ),
    declaration(
        LocalMemoryApi::ReAlloc,
        "GlobalReAlloc",
        &[P, P, D],
        H,
        false,
    ),
    declaration(LocalMemoryApi::Free, "LocalFree", &[P], H, true),
    declaration(LocalMemoryApi::Free, "GlobalFree", &[P], H, true),
    declaration(LocalMemoryApi::Size, "LocalSize", &[P], H, true),
    declaration(LocalMemoryApi::Size, "GlobalSize", &[P], H, true),
    declaration(LocalMemoryApi::Lock, "LocalLock", &[P], H, true),
    declaration(LocalMemoryApi::Lock, "GlobalLock", &[P], H, true),
    declaration(LocalMemoryApi::Unlock, "LocalUnlock", &[P], B, true),
    declaration(LocalMemoryApi::Unlock, "GlobalUnlock", &[P], B, true),
    declaration(LocalMemoryApi::Flags, "LocalFlags", &[P], U, false),
    declaration(LocalMemoryApi::Flags, "GlobalFlags", &[P], U, false),
];
