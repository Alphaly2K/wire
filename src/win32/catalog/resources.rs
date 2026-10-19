//! PE32 resource views over the mapped guest image; Win64 remains absent.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceApi {
    LoadStringA,
    LoadStringW,
    FindA,
    FindW,
    FindExA,
    FindExW,
    Load,
    Size,
    Lock,
    Free,
}

const fn declaration(
    id: ResourceApi,
    name: &'static str,
    dlls: &'static [&'static str],
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Resource(id), name, parameters, returns,
        "PE32 mapped-image resource views; null module uses exe base; preserve numeric/named key decoding, ASCII name folding, full DWORD language carrier and first-language fallback; LoadString masks the ID to WORD, retains CP932 byte truncation, W zero-capacity pointer output and partial writes before faults; only FindResource miss sets LastError 1814, LockResource is identity and FreeResource returns zero; output precedes gateway return-frame read; Win64 absent",
        [ArchitecturePlan { dlls, execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }],
    )
}

use Parameter::{Int32 as I, Pointer as P, Uint32 as U};
use ReturnConvention::{BoolLastError as B, CharacterCount as C, Pointer as H, Uint32 as D};
pub static RESOURCE_CONTRACTS: [Contract; 10] = [
    declaration(
        ResourceApi::LoadStringA,
        "LoadStringA",
        &["user32.dll"],
        &[P, U, P, I],
        C,
    ),
    declaration(
        ResourceApi::LoadStringW,
        "LoadStringW",
        &["user32.dll"],
        &[P, U, P, I],
        C,
    ),
    declaration(
        ResourceApi::FindA,
        "FindResourceA",
        &["kernel32.dll"],
        &[P, P, P],
        H,
    ),
    declaration(
        ResourceApi::FindW,
        "FindResourceW",
        &["kernel32.dll"],
        &[P, P, P],
        H,
    ),
    declaration(
        ResourceApi::FindExA,
        "FindResourceExA",
        &["kernel32.dll"],
        &[P, P, P, U],
        H,
    ),
    declaration(
        ResourceApi::FindExW,
        "FindResourceExW",
        &["kernel32.dll"],
        &[P, P, P, U],
        H,
    ),
    declaration(
        ResourceApi::Load,
        "LoadResource",
        &["kernel32.dll"],
        &[P, P],
        H,
    ),
    declaration(
        ResourceApi::Size,
        "SizeofResource",
        &["kernel32.dll"],
        &[P, P],
        D,
    ),
    declaration(
        ResourceApi::Lock,
        "LockResource",
        &["kernel32.dll"],
        &[P],
        H,
    ),
    declaration(
        ResourceApi::Free,
        "FreeResource",
        &["kernel32.dll"],
        &[P],
        B,
    ),
];
