use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CriticalSectionApi {
    Initialize,
    InitializeSpin,
    InitializeEx,
    Enter,
    TryEnter,
    Leave,
    Delete,
    SetSpin,
}

const fn declaration(
    id: CriticalSectionApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::CriticalSection(id), name, parameters, returns,
        "cooperative recursive critical sections; PE32 sequential 24-byte field writes, permissive leave/delete, mutation before return fault, SetSpin ABI-only; Win64 checked 40-byte layout, ownership/overflow validation, DWORD inputs and historical full spin return, saved return frame, failed Try without unblock; scheduling remains in Process",
        [
            ArchitecturePlan { dlls: &["kernel32.dll"], execution: if matches!(id, CriticalSectionApi::SetSpin) { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Typed } },
            ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Uint32 as U, Void as V};
pub static CRITICAL_SECTION_CONTRACTS: [Contract; 8] = [
    declaration(
        CriticalSectionApi::Initialize,
        "InitializeCriticalSection",
        &[P],
        V,
    ),
    declaration(
        CriticalSectionApi::InitializeSpin,
        "InitializeCriticalSectionAndSpinCount",
        &[P, D],
        B,
    ),
    declaration(
        CriticalSectionApi::InitializeEx,
        "InitializeCriticalSectionEx",
        &[P, D, D],
        B,
    ),
    declaration(CriticalSectionApi::Enter, "EnterCriticalSection", &[P], V),
    declaration(
        CriticalSectionApi::TryEnter,
        "TryEnterCriticalSection",
        &[P],
        B,
    ),
    declaration(CriticalSectionApi::Leave, "LeaveCriticalSection", &[P], V),
    declaration(CriticalSectionApi::Delete, "DeleteCriticalSection", &[P], V),
    declaration(
        CriticalSectionApi::SetSpin,
        "SetCriticalSectionSpinCount",
        &[P, D],
        U,
    ),
];
