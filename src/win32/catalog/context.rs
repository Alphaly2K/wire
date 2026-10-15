use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextApi {
    ProcessId,
    ThreadId,
    ProcessHandle,
    ThreadHandle,
    DebuggerPresent,
    GetLastError,
    SetLastError,
}

const fn declaration(
    id: ContextApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Context(id),
        name,
        parameters,
        returns,
        "existing architecture-specific process IDs and full-width pseudo handles; scheduler-owned thread identity; PE32 TEB error import/export and commit-before-return-fault; Win64 TEB error, DWORD argument, PEB debug byte and original hot/general frame preflight",
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

use ReturnConvention::{Pointer as P, Uint32 as D, Void};

pub static CONTEXT_CONTRACTS: [Contract; 7] = [
    declaration(ContextApi::ProcessId, "GetCurrentProcessId", &[], D),
    declaration(ContextApi::ThreadId, "GetCurrentThreadId", &[], D),
    declaration(ContextApi::ProcessHandle, "GetCurrentProcess", &[], P),
    declaration(ContextApi::ThreadHandle, "GetCurrentThread", &[], P),
    declaration(ContextApi::DebuggerPresent, "IsDebuggerPresent", &[], D),
    declaration(ContextApi::GetLastError, "GetLastError", &[], D),
    declaration(
        ContextApi::SetLastError,
        "SetLastError",
        &[Parameter::Uint32],
        Void,
    ),
];
