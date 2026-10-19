//! Process entry/exit controls retain each architecture's explicit outcomes.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessControlApi {
    Create,
    Exit,
}
const fn declaration(
    id: ProcessControlApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(ApiId::ProcessControl(id), name, parameters, returns,
        "PE32 CreateProcessW ABI-only, ExitProcess retains DLL detach callbacks and halting without normal return; Win64 child startup preflight, invalid-cb direct completion and missing-executable diagnostics, existing child execution rejection; exit retains saved frame validation, explicit process outcome and trace/API-count order",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: if matches!(id, ProcessControlApi::Create) { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Typed } },
         ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }])
}
use Parameter::{Pointer as P, Uint32 as D};
pub static PROCESS_CONTROL_CONTRACTS: [Contract; 2] = [
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
];
