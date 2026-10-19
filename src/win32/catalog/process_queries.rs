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
pub static PROCESS_QUERY_CONTRACTS: [Contract; 11] = [
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
