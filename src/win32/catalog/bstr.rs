use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BstrApi {
    Allocate,
    Reallocate,
    AllocateLength,
    ReallocateLength,
    Free,
    Length,
    ByteLength,
    AllocateBytes,
}

// Win64's existing explicit ordinal aliases. PE32 keeps Wine spec resolution.
pub(crate) fn symbol<'a>(dll: &str, name: &'a str) -> &'a str {
    if !dll.eq_ignore_ascii_case("oleaut32.dll") {
        return name;
    }
    match name {
        "#2" => "SysAllocString",
        "#3" => "SysReAllocString",
        "#4" => "SysAllocStringLen",
        "#5" => "SysReAllocStringLen",
        "#6" => "SysFreeString",
        "#7" => "SysStringLen",
        "#149" => "SysStringByteLen",
        "#150" => "SysAllocStringByteLen",
        _ => name,
    }
}

const fn declaration(
    id: BstrApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Bstr(id), name, parameters, returns,
        "existing BSTR byte prefix and ownership: length queries accept readable foreign prefixes; PE32 shares process heap and Variant/WMI, bounded UTF-16 units, original LastError and late return frame, byte allocation ABI-only; Win64 DWORD lengths/full pointers, strict allocation membership, byte allocation, alias source read and saved return frame; output overlap and failure order retained",
        [
            ArchitecturePlan { dlls: &["oleaut32.dll"], execution: if matches!(id, BstrApi::AllocateBytes) { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Typed } },
            ArchitecturePlan { dlls: &["oleaut32.dll"], execution: ExecutionPolicy::Typed },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{Pointer as R, Uint32 as U, Void as V};
pub static BSTR_CONTRACTS: [Contract; 8] = [
    declaration(BstrApi::Allocate, "SysAllocString", &[P], R),
    declaration(BstrApi::Reallocate, "SysReAllocString", &[P, P], U),
    declaration(BstrApi::AllocateLength, "SysAllocStringLen", &[P, D], R),
    declaration(
        BstrApi::ReallocateLength,
        "SysReAllocStringLen",
        &[P, P, D],
        U,
    ),
    declaration(BstrApi::Free, "SysFreeString", &[P], V),
    declaration(BstrApi::Length, "SysStringLen", &[P], U),
    declaration(BstrApi::ByteLength, "SysStringByteLen", &[P], U),
    declaration(BstrApi::AllocateBytes, "SysAllocStringByteLen", &[P, D], R),
];
