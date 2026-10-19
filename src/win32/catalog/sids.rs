//! Guest SID bytes and views, never a host credential representation.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidApi {
    SubAuthority,
    SubAuthorityCount,
    IdentifierAuthority,
    Valid,
    Length,
    Copy,
    Equal,
    LengthRequired,
    Initialize,
}

const fn declaration(
    id: SidApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Sid(id), name, parameters, returns,
        "PE32 advapi32 ABI-only zero/error 120, untouched output and stdcall cleanup; Win64 static advapi32/kernel32/kernelbase imports with dynamic lookup only after gateway registration; checked guest SID revision 1/count <=15, full pointers, DWORD subauthority index/copy capacity, BYTE count consumed without truncating trace carrier, header-only initialization, destination-before-authority preflight and repeated CopySid source read/guard consumption, error 87/122/1337 and saved return frame preserved",
        [ArchitecturePlan { dlls: &["advapi32.dll"], execution: ExecutionPolicy::LegacyUnimplemented },
         ArchitecturePlan { dlls: &["advapi32.dll", "kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }],
    ).with_lookup_dlls([&["advapi32.dll"], &[]])
}

use Parameter::{Pointer as P, Uint32 as D, Uint8 as C};
use ReturnConvention::{BoolLastError as B, Pointer as R, Uint32 as U};
pub static SID_CONTRACTS: [Contract; 9] = [
    declaration(SidApi::SubAuthority, "GetSidSubAuthority", &[P, D], R),
    declaration(
        SidApi::SubAuthorityCount,
        "GetSidSubAuthorityCount",
        &[P],
        R,
    ),
    declaration(
        SidApi::IdentifierAuthority,
        "GetSidIdentifierAuthority",
        &[P],
        R,
    ),
    declaration(SidApi::Valid, "IsValidSid", &[P], B),
    declaration(SidApi::Length, "GetLengthSid", &[P], U),
    declaration(SidApi::Copy, "CopySid", &[D, P, P], B),
    declaration(SidApi::Equal, "EqualSid", &[P, P], B),
    declaration(SidApi::LengthRequired, "GetSidLengthRequired", &[C], U),
    declaration(SidApi::Initialize, "InitializeSid", &[P, P, C], B),
];
