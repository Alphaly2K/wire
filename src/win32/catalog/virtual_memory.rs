use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtualMemoryApi {
    Alloc,
    AllocEx,
    Free,
    FreeEx,
    Protect,
    ProtectEx,
    Query,
    QueryEx,
}

impl VirtualMemoryApi {
    pub(crate) fn extended(self) -> bool {
        matches!(
            self,
            Self::AllocEx | Self::FreeEx | Self::ProtectEx | Self::QueryEx
        )
    }
    pub(crate) fn base(self) -> Self {
        match self {
            Self::AllocEx => Self::Alloc,
            Self::FreeEx => Self::Free,
            Self::ProtectEx => Self::Protect,
            Self::QueryEx => Self::Query,
            other => other,
        }
    }
}

const fn declaration(
    id: VirtualMemoryApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::VirtualMemory(id), name, parameters, returns,
        "current-process-only checked virtual memory; preserve separate memory/cache owners, 28/48-byte query layouts and length/errors, PE32 output errors and unsaved return-slot alias versus Win64 saved return and checked outputs before protection; SIZE_T and handles keep architecture width, DWORD flags truncate",
        [
            ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::Typed },
            ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed },
        ],
    )
}

// Pointer is also the architecture-width carrier for SIZE_T/uintptr_t.
use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as H};

pub static VIRTUAL_MEMORY_CONTRACTS: [Contract; 8] = [
    declaration(VirtualMemoryApi::Alloc, "VirtualAlloc", &[P, P, D, D], H),
    declaration(
        VirtualMemoryApi::AllocEx,
        "VirtualAllocEx",
        &[P, P, P, D, D],
        H,
    ),
    declaration(VirtualMemoryApi::Free, "VirtualFree", &[P, P, D], B),
    declaration(VirtualMemoryApi::FreeEx, "VirtualFreeEx", &[P, P, P, D], B),
    declaration(
        VirtualMemoryApi::Protect,
        "VirtualProtect",
        &[P, P, D, P],
        B,
    ),
    declaration(
        VirtualMemoryApi::ProtectEx,
        "VirtualProtectEx",
        &[P, P, P, D, P],
        B,
    ),
    declaration(VirtualMemoryApi::Query, "VirtualQuery", &[P, P, P], H),
    declaration(
        VirtualMemoryApi::QueryEx,
        "VirtualQueryEx",
        &[P, P, P, P],
        H,
    ),
];
