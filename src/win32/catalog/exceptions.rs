//! Exception controls and metadata registration retain architecture-specific policies.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExceptionApi {
    AddVectored,
    RemoveVectored,
    Raise,
    SetFilter,
    UnhandledFilter,
    Unwind,
    UnwindEx,
    AddTable,
    DeleteTable,
    InstallCallback,
    LookupEntry,
    CaptureContext,
    PcToFileHeader,
    VirtualUnwind,
}

const fn declaration(
    id: ExceptionApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    use ExceptionApi::*;
    let pe32 = matches!(
        id,
        AddVectored | RemoveVectored | Raise | SetFilter | UnhandledFilter | Unwind
    );
    let native = matches!(
        id,
        UnwindEx | AddTable | DeleteTable | InstallCallback | LookupEntry
    );
    Contract::new(
        ApiId::Exception(id), name, parameters, returns,
        "existing software SEH and immediate-frame AMD64 unwind; PE32 vectored registration ABI-only, architecture-specific unknown providers retained; Win64 ordered vectored handlers and monotonic tokens, checked function-table metadata and process-only guest lookup callbacks; saved return preflight, original CPU/trace/error submission and explicit raise/unwind control outcomes",
        [ArchitecturePlan {
            dlls: if matches!(id, Unwind) { &["kernel32.dll", "ntdll.dll"] } else if pe32 { &["kernel32.dll"] } else { &[] },
            execution: if matches!(id, AddVectored | RemoveVectored) { ExecutionPolicy::LegacyUnimplemented } else if pe32 { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent },
        }, ArchitecturePlan {
            dlls: if native { &["kernel32.dll", "kernelbase.dll", "ntdll.dll"] } else if matches!(id, UnhandledFilter | Unwind) { &[] } else { &["kernel32.dll", "kernelbase.dll"] },
            execution: if matches!(id, UnhandledFilter | Unwind) { ExecutionPolicy::Absent } else { ExecutionPolicy::Typed },
        }],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{Pointer as R, Uint32 as U, Void as V};
pub static EXCEPTION_CONTRACTS: [Contract; 14] = [
    Contract::new(
        ApiId::Exception(ExceptionApi::PcToFileHeader),
        "RtlPcToFileHeader",
        &[P, P],
        R,
        "returns the mapped image base containing an address and writes BaseOfImage",
        [ArchitecturePlan {
            dlls: &[],
            execution: ExecutionPolicy::Absent,
        }; 2],
    ),
    Contract::new(
        ApiId::Exception(ExceptionApi::VirtualUnwind),
        "RtlVirtualUnwind",
        &[D, P, P, P, P, P, P, P],
        R,
        "unwinds a supplied AMD64 CONTEXT with optional saved-register addresses and returns a matching language handler",
        [ArchitecturePlan {
            dlls: &[],
            execution: ExecutionPolicy::Absent,
        }; 2],
    ),
    Contract::new(
        ApiId::Exception(ExceptionApi::CaptureContext),
        "RtlCaptureContext",
        &[P],
        V,
        "captures the caller processor context into a guest CONTEXT record",
        [ArchitecturePlan {
            dlls: &[],
            execution: ExecutionPolicy::Absent,
        }; 2],
    ),
    declaration(
        ExceptionApi::AddVectored,
        "AddVectoredExceptionHandler",
        &[D, P],
        R,
    ),
    declaration(
        ExceptionApi::RemoveVectored,
        "RemoveVectoredExceptionHandler",
        &[P],
        U,
    ),
    declaration(ExceptionApi::Raise, "RaiseException", &[D, D, D, P], V),
    declaration(
        ExceptionApi::SetFilter,
        "SetUnhandledExceptionFilter",
        &[P],
        R,
    ),
    declaration(
        ExceptionApi::UnhandledFilter,
        "UnhandledExceptionFilter",
        &[P],
        U,
    ),
    declaration(ExceptionApi::Unwind, "RtlUnwind", &[P, P, P, P], V),
    declaration(
        ExceptionApi::UnwindEx,
        "RtlUnwindEx",
        &[P, P, P, P, P, P],
        V,
    ),
    declaration(ExceptionApi::AddTable, "RtlAddFunctionTable", &[P, D, P], U),
    declaration(ExceptionApi::DeleteTable, "RtlDeleteFunctionTable", &[P], U),
    declaration(
        ExceptionApi::InstallCallback,
        "RtlInstallFunctionTableCallback",
        &[P, P, D, P, P, P],
        U,
    ),
    declaration(
        ExceptionApi::LookupEntry,
        "RtlLookupFunctionEntry",
        &[P, P, P],
        R,
    ),
];
