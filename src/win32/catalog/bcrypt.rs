//! BCrypt currently exposes only the OS-backed guest RNG provider subset.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BcryptApi {
    Generate,
    Open,
    Close,
    SystemRandom,
}

const fn declaration(
    id: BcryptApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(
        ApiId::Bcrypt(id), name, parameters, ReturnConvention::Ntstatus,
        "PE32 absent/unknown ABI; Win64 static bcrypt/kernel32/kernelbase, dynamic lookup only after gateway registration; RNG and optional Microsoft Primitive Provider only, 1024 live full-width reusable handles, pseudo-handle 0x81, system-preferred flag and DWORD flags/length; 16MiB cap, output preflight before OS entropy or provider capacity, 256-byte entropy chunks, NTSTATUS and unchanged LastError, original string/validation/commit order",
        [ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent },
         ArchitecturePlan { dlls: &["bcrypt.dll", "kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }],
    ).with_lookup_dlls([&[], &[]])
}

use Parameter::{Pointer as P, Uint32 as D};
pub static BCRYPT_CONTRACTS: [Contract; 4] = [
    declaration(BcryptApi::Generate, "BCryptGenRandom", &[P, P, D, D]),
    declaration(
        BcryptApi::Open,
        "BCryptOpenAlgorithmProvider",
        &[P, P, P, D],
    ),
    declaration(BcryptApi::Close, "BCryptCloseAlgorithmProvider", &[P, D]),
    Contract::new(
        ApiId::Bcrypt(BcryptApi::SystemRandom),
        "SystemFunction036",
        &[P, D],
        ReturnConvention::Uint32,
        "BOOLEAN RtlGenRandom export; shared secure entropy sample, DWORD byte length, complete output preflight, zero length succeeds, 16 MiB request cap and unchanged LastError; no provider handle",
        [ArchitecturePlan { dlls: &["advapi32.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2],
    ),
];
