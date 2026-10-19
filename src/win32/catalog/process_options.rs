//! Process error policy and owned startup attribute lists.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessOptionApi {
    GetErrorMode,
    SetErrorMode,
    InitializeAttributes,
    UpdateAttribute,
    DeleteAttributes,
}

const fn declaration(
    id: ProcessOptionApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::ProcessOption(id), name, parameters, returns,
        "PE32 retains kernel32 ABI-only policy from Wine spec; Win64 kernel32/kernelbase static/dynamic providers, process error-mode mask, full-width owned attribute lists, original query/copyout/insert and flags/handle validation order; only inherited pipe/event handle-list attribute is supported, no child-process backend",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::LegacyUnimplemented },
         ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Uint32 as U, Void as V};
pub static PROCESS_OPTION_CONTRACTS: [Contract; 5] = [
    declaration(ProcessOptionApi::GetErrorMode, "GetErrorMode", &[], U),
    declaration(ProcessOptionApi::SetErrorMode, "SetErrorMode", &[D], U),
    declaration(
        ProcessOptionApi::InitializeAttributes,
        "InitializeProcThreadAttributeList",
        &[P, D, D, P],
        B,
    ),
    declaration(
        ProcessOptionApi::UpdateAttribute,
        "UpdateProcThreadAttribute",
        &[P, D, P, P, P, P, P],
        B,
    ),
    declaration(
        ProcessOptionApi::DeleteAttributes,
        "DeleteProcThreadAttributeList",
        &[P],
        V,
    ),
];
