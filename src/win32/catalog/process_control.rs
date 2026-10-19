//! Process entry/exit controls retain each architecture's explicit outcomes.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessControlApi {
    Create,
    Exit,
    Terminate,
}
const fn declaration(
    id: ProcessControlApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(ApiId::ProcessControl(id), name, parameters, returns,
        "PE32 CreateProcessW ABI-only, ExitProcess retains DLL detach callbacks and halting without normal return; TerminateProcess accepts the pseudo handle or Process object, halts before the ordinary return-frame read without DLL detach; Win64 TerminateProcess absent; child startup preflight, invalid-cb direct completion and missing-executable diagnostics, existing child execution rejection; exit retains saved frame validation, explicit process outcome and trace/API-count order",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: if matches!(id, ProcessControlApi::Create) { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Typed } },
         ArchitecturePlan { dlls: if matches!(id, ProcessControlApi::Terminate) { &[] } else { &["kernel32.dll", "kernelbase.dll"] }, execution: if matches!(id, ProcessControlApi::Terminate) { ExecutionPolicy::Absent } else { ExecutionPolicy::Typed } }])
}
use Parameter::{Pointer as P, Uint32 as D};
pub static PROCESS_CONTROL_CONTRACTS: [Contract; 3] = [
    declaration(
        ProcessControlApi::Create,
        "CreateProcessW",
        &[P, P, P, P, D, D, P, P, P, P],
        ReturnConvention::BoolLastError,
    ),
    declaration(
        ProcessControlApi::Exit,
        "ExitProcess",
        &[D],
        ReturnConvention::Void,
    ),
    declaration(
        ProcessControlApi::Terminate,
        "TerminateProcess",
        &[P, D],
        ReturnConvention::BoolLastError,
    ),
];
