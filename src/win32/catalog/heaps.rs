use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeapApi {
    Process,
    Create,
    Destroy,
    Alloc,
    Free,
    ReAlloc,
    Size,
    Validate,
    Enumerate,
    SetInformation,
    QueryInformation,
    Compact,
    Walk,
}

const fn declaration(
    id: HeapApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
    pe32_typed: bool,
    win64_typed: bool,
) -> Contract {
    Contract::new(
        ApiId::Heap(id), name, parameters, returns,
        "existing separate heap owners and allocators; PE32 private heaps, LastError, silent enumeration copy-out fault and unsaved return alias; four extensions remain ABI-only; Win64 current process heap and flags subset, full-width handles/SIZE_T, checked return frame and invalid block faults; other exports absent",
        [
            ArchitecturePlan {
                dlls: &["kernel32.dll"],
                execution: if pe32_typed { ExecutionPolicy::Typed } else { ExecutionPolicy::LegacyUnimplemented },
            },
            ArchitecturePlan {
                dlls: if win64_typed { &["kernel32.dll", "kernelbase.dll"] } else { &[] },
                execution: if win64_typed { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent },
            },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as H, Uint32 as U};

pub static HEAP_CONTRACTS: [Contract; 13] = [
    declaration(HeapApi::Process, "GetProcessHeap", &[], H, true, true),
    declaration(HeapApi::Create, "HeapCreate", &[D, P, P], H, true, false),
    declaration(HeapApi::Destroy, "HeapDestroy", &[P], B, true, false),
    declaration(HeapApi::Alloc, "HeapAlloc", &[P, D, P], H, true, true),
    declaration(HeapApi::Free, "HeapFree", &[P, D, P], B, true, true),
    declaration(
        HeapApi::ReAlloc,
        "HeapReAlloc",
        &[P, D, P, P],
        H,
        true,
        true,
    ),
    declaration(HeapApi::Size, "HeapSize", &[P, D, P], H, true, true),
    declaration(
        HeapApi::Validate,
        "HeapValidate",
        &[P, D, P],
        B,
        true,
        false,
    ),
    declaration(
        HeapApi::Enumerate,
        "GetProcessHeaps",
        &[D, P],
        U,
        true,
        false,
    ),
    declaration(
        HeapApi::SetInformation,
        "HeapSetInformation",
        &[P, D, P, P],
        B,
        false,
        false,
    ),
    declaration(
        HeapApi::QueryInformation,
        "HeapQueryInformation",
        &[P, D, P, P, P],
        B,
        false,
        false,
    ),
    declaration(HeapApi::Compact, "HeapCompact", &[P, D], H, false, false),
    declaration(HeapApi::Walk, "HeapWalk", &[P, P], B, false, false),
];
