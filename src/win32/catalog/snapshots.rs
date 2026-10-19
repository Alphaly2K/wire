//! Toolhelp is ABI-only on PE32 and a confined guest snapshot on Win64.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotApi {
    Create,
    ProcessFirstA,
    ProcessNextA,
    ProcessFirstW,
    ProcessNextW,
    ThreadFirst,
    ThreadNext,
}

const fn declaration(
    id: SnapshotApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Snapshot(id), name, parameters, returns,
        "PE32 preserves known stdcall ABI, unavailable return zero/error 120 and untouched outputs; Win64 snapshots contain only guest process/thread values captured at creation, ignore PID, preserve 128 live/4096 lifetime limits, DWORD creation flags, full pointer-width handle identity, ANSI/wide shared process cursor, size-read/copyout/fault order, saved return frame, LastError and protected close error 5; no Process32FirstA/NextA alias",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::LegacyUnimplemented },
         ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as H};
pub static SNAPSHOT_CONTRACTS: [Contract; 7] = [
    declaration(SnapshotApi::Create, "CreateToolhelp32Snapshot", &[D, D], H),
    declaration(SnapshotApi::ProcessFirstA, "Process32First", &[P, P], B),
    declaration(SnapshotApi::ProcessNextA, "Process32Next", &[P, P], B),
    declaration(SnapshotApi::ProcessFirstW, "Process32FirstW", &[P, P], B),
    declaration(SnapshotApi::ProcessNextW, "Process32NextW", &[P, P], B),
    declaration(SnapshotApi::ThreadFirst, "Thread32First", &[P, P], B),
    declaration(SnapshotApi::ThreadNext, "Thread32Next", &[P, P], B),
];
