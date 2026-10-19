//! Disabled ETW providers expose status results, without changing LastError.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtwApi {
    Register,
    Unregister,
    Enabled,
    ProviderEnabled,
    Write,
    WriteString,
    WriteTransfer,
    SetInformation,
}

const fn declaration(id: EtwApi, name: &'static str, parameters: &'static [Parameter]) -> Contract {
    Contract::new(
        ApiId::Etw(id), name, parameters, ReturnConvention::Uint32,
        "PE32 advapi32 ABI-only zero/error 120 and untouched output; REGHANDLE/keyword consume two DWORD stack slots; Win64 static advapi32/kernel32/kernelbase imports, dynamic lookup requires registered gateway, full U64 handles/keywords and raw BYTE trace carrier, DWORD count/info/length, 64 live/4096 lifetime registrations, source/output checks before capacity, saved frame, status return with unchanged LastError; disabled events check descriptor/count arrays but never consume payloads, activity IDs, strings or callbacks",
        [ArchitecturePlan { dlls: &["advapi32.dll"], execution: ExecutionPolicy::LegacyUnimplemented },
         ArchitecturePlan { dlls: &["advapi32.dll", "kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }],
    ).with_lookup_dlls([&["advapi32.dll"], &[]])
}

use Parameter::{Pointer as P, Uint32 as D, Uint64 as H, Uint8 as B};
pub static ETW_CONTRACTS: [Contract; 8] = [
    declaration(EtwApi::Register, "EventRegister", &[P, P, P, P]),
    declaration(EtwApi::Unregister, "EventUnregister", &[H]),
    declaration(EtwApi::Enabled, "EventEnabled", &[H, P]),
    declaration(EtwApi::ProviderEnabled, "EventProviderEnabled", &[H, B, H]),
    declaration(EtwApi::Write, "EventWrite", &[H, P, D, P]),
    declaration(EtwApi::WriteString, "EventWriteString", &[H, B, H, P]),
    declaration(
        EtwApi::WriteTransfer,
        "EventWriteTransfer",
        &[H, P, P, P, D, P],
    ),
    declaration(EtwApi::SetInformation, "EventSetInformation", &[H, D, P, D]),
];
