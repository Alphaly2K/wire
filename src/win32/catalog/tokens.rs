//! Query-only token identity, preserving legacy static/dynamic provider scopes.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenApi {
    OpenProcess,
    OpenThread,
    Information,
}

const fn declaration(
    id: TokenApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    let x86_dlls: &[&str] = if matches!(id, TokenApi::Information) {
        &["advapi32.dll"]
    } else {
        &["advapi32.dll", "kernel32.dll"]
    };
    Contract::new(
        ApiId::Token(id), name, parameters, ReturnConvention::BoolLastError,
        "PE32 ABI-only zero/error 120, untouched outputs and provider-specific stdcall cleanup; Win64 static advapi32/kernel32/kernelbase imports, dynamic lookup only after gateway registration, query-only confined guest identity, full-width process/thread/token handles, DWORD access/BOOL/class/length, 128 live/4096 lifetime tokens, validation/copyout ordering, saved return frame and protected close error 6 preserved; OpenThreadToken returns 1008 for a valid guest thread and 6 otherwise without reading its output pointer",
        [ArchitecturePlan { dlls: x86_dlls, execution: ExecutionPolicy::LegacyUnimplemented },
         ArchitecturePlan { dlls: &["advapi32.dll", "kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }],
    ).with_lookup_dlls([x86_dlls, &[]])
}

use Parameter::{Pointer as P, Uint32 as D};
pub static TOKEN_CONTRACTS: [Contract; 3] = [
    declaration(TokenApi::OpenProcess, "OpenProcessToken", &[P, D, P]),
    declaration(TokenApi::OpenThread, "OpenThreadToken", &[P, D, D, P]),
    declaration(
        TokenApi::Information,
        "GetTokenInformation",
        &[P, D, P, D, P],
    ),
];
