use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessQueryApi {
    Open,
    Affinity,
    SetAffinity,
    SetIdealProcessor,
    Id,
    ExitCode,
    ActiveProcessorCount,
    MaximumProcessorCount,
    ActiveGroupCount,
    MaximumGroupCount,
    PackageId,
    MemoryInfo,
    Priority,
    SetPriority,
    Times,
    WorkingSet,
    SetWorkingSet,
    Enumerate,
    InputIdle,
}

const fn declaration(
    id: ProcessQueryApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let counts = matches!(
        id,
        ProcessQueryApi::ActiveProcessorCount | ProcessQueryApi::MaximumProcessorCount
    );
    let groups = matches!(
        id,
        ProcessQueryApi::ActiveGroupCount | ProcessQueryApi::MaximumGroupCount
    );
    Contract::new(ApiId::ProcessQuery(id), name, parameters, returns,
        "existing fixed PID/single-CPU profile, PE32 unpackaged-app status without outputs or LastError mutation, handle access checks before output pointers, process opening delegates handle ownership to gateway, ignored inherit flag, full group carrier validation and UINT_MAX ideal-processor failure, group-count ABI-only; Win64 only processor/group counts, low WORD consumed while preserving raw trace carrier and saved frame",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: if groups { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Typed } },
         ArchitecturePlan { dlls: if counts || groups { &["kernel32.dll", "kernelbase.dll"] } else { &[] }, execution: if counts || groups { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } }])
}

use Parameter::{Pointer as P, Uint16 as W, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as R, Uint32 as U};
pub static PROCESS_QUERY_CONTRACTS: [Contract; 21] = [
    support(
        ProcessQueryApi::Priority,
        "GetPriorityClass",
        &[P],
        U,
        &["kernel32.dll", "kernelbase.dll"],
    ),
    support(
        ProcessQueryApi::SetPriority,
        "SetPriorityClass",
        &[P, D],
        B,
        &["kernel32.dll", "kernelbase.dll"],
    ),
    support(
        ProcessQueryApi::Times,
        "GetProcessTimes",
        &[P, P, P, P, P],
        B,
        &["kernel32.dll", "kernelbase.dll"],
    ),
    support(
        ProcessQueryApi::WorkingSet,
        "GetProcessWorkingSetSize",
        &[P, P, P],
        B,
        &["kernel32.dll", "kernelbase.dll"],
    ),
    support(
        ProcessQueryApi::SetWorkingSet,
        "SetProcessWorkingSetSize",
        &[P, P, P],
        B,
        &["kernel32.dll", "kernelbase.dll"],
    ),
    support(
        ProcessQueryApi::Enumerate,
        "K32EnumProcesses",
        &[P, D, P],
        B,
        &["kernel32.dll", "kernelbase.dll"],
    ),
    support(
        ProcessQueryApi::Enumerate,
        "EnumProcesses",
        &[P, D, P],
        B,
        &["psapi.dll"],
    ),
    support(
        ProcessQueryApi::InputIdle,
        "WaitForInputIdle",
        &[P, D],
        U,
        &["user32.dll"],
    ),
    memory_info("GetProcessMemoryInfo", &["psapi.dll"]),
    memory_info(
        "K32GetProcessMemoryInfo",
        &["kernel32.dll", "kernelbase.dll"],
    ),
    declaration(
        ProcessQueryApi::PackageId,
        "GetCurrentPackageId",
        &[P, P],
        U,
    ),
    declaration(ProcessQueryApi::Open, "OpenProcess", &[D, D, D], R),
    declaration(
        ProcessQueryApi::Affinity,
        "GetProcessAffinityMask",
        &[P, P, P],
        B,
    ),
    declaration(
        ProcessQueryApi::SetAffinity,
        "SetProcessAffinityMask",
        &[P, P],
        B,
    ),
    declaration(
        ProcessQueryApi::SetIdealProcessor,
        "SetThreadIdealProcessor",
        &[P, D],
        U,
    ),
    declaration(ProcessQueryApi::Id, "GetProcessId", &[P], U),
    declaration(ProcessQueryApi::ExitCode, "GetExitCodeProcess", &[P, P], B),
    declaration(
        ProcessQueryApi::ActiveProcessorCount,
        "GetActiveProcessorCount",
        &[W],
        U,
    ),
    declaration(
        ProcessQueryApi::MaximumProcessorCount,
        "GetMaximumProcessorCount",
        &[W],
        U,
    ),
    declaration(
        ProcessQueryApi::ActiveGroupCount,
        "GetActiveProcessorGroupCount",
        &[],
        U,
    ),
    declaration(
        ProcessQueryApi::MaximumGroupCount,
        "GetMaximumProcessorGroupCount",
        &[],
        U,
    ),
];

const fn memory_info(name: &'static str, dlls: &'static [&'static str]) -> Contract {
    Contract::new(
        ApiId::ProcessQuery(ProcessQueryApi::MemoryInfo), name, &[P, P, D], B,
        "current-process live VMM counters and retained peaks; 40/72-byte base and 44/80-byte extended layouts, short buffers fail with 122 before handle checks; QUERY or QUERY_LIMITED access; mapped/image pages excluded from private commit; no paging or guest kernel pool; full output preflight, success preserves LastError; EX2 unavailable",
        [ArchitecturePlan { dlls, execution: ExecutionPolicy::Typed }; 2],
    )
}

const fn support(
    id: ProcessQueryApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
    dlls: &'static [&'static str],
) -> Contract {
    Contract::new(ApiId::ProcessQuery(id),name,parameters,returns,"Existing virtual process owner, access and identity; priority and process-family enumeration are shared state; unavailable CPU accounting, working-set controls or input-idle monitoring fail explicitly",[ArchitecturePlan {dlls,execution:ExecutionPolicy::LegacyUnimplemented};2])
}
