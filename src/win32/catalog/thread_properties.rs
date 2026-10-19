use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadPropertyApi {
    GetErrorMode,
    SetErrorMode,
    Resume,
    Suspend,
    Id,
    GetPriority,
    SetPriority,
    ExitCode,
    SetAffinity,
    StackGuarantee,
}

const fn declaration(
    id: ThreadPropertyApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
    pe32_typed: bool,
) -> Contract {
    Contract::new(
        ApiId::ThreadProperty(id), name, parameters, returns,
        "existing cooperative thread owners; PE32 two-state suspension, priority no-op, unknown exit handle writes zero, unsaved return slot; ID/error-mode/stack guarantee ABI-only; Win64 full-width handles/affinity, DWORD priority/mode, counted suspension capped at 127, identity before output, mode copy-out before update and saved return frame; no stack reserve, zero query writes zero, nonzero guarantee preserves input and returns error 50",
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

use Parameter::{Int32 as I, Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as H, Uint32 as U};

pub static THREAD_PROPERTY_CONTRACTS: [Contract; 10] = [
    declaration(
        ThreadPropertyApi::GetErrorMode,
        "GetThreadErrorMode",
        &[],
        U,
        false,
    ),
    declaration(
        ThreadPropertyApi::SetErrorMode,
        "SetThreadErrorMode",
        &[D, P],
        B,
        false,
    ),
    declaration(ThreadPropertyApi::Resume, "ResumeThread", &[P], U, true),
    declaration(ThreadPropertyApi::Suspend, "SuspendThread", &[P], U, true),
    declaration(ThreadPropertyApi::Id, "GetThreadId", &[P], U, false),
    declaration(
        ThreadPropertyApi::GetPriority,
        "GetThreadPriority",
        &[P],
        U,
        true,
    ),
    declaration(
        ThreadPropertyApi::SetPriority,
        "SetThreadPriority",
        &[P, I],
        B,
        true,
    ),
    declaration(
        ThreadPropertyApi::ExitCode,
        "GetExitCodeThread",
        &[P, P],
        B,
        true,
    ),
    declaration(
        ThreadPropertyApi::SetAffinity,
        "SetThreadAffinityMask",
        &[P, P],
        H,
        true,
    ),
    declaration(
        ThreadPropertyApi::StackGuarantee,
        "SetThreadStackGuarantee",
        &[P],
        B,
        false,
    ),
];
