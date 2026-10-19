use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerHelperApi {
    BadRead,
    BadWrite,
    BadCode,
    FlushCache,
    MulDiv,
    Encode,
    Decode,
}

impl PointerHelperApi {
    pub(crate) const fn encoding(self) -> bool {
        matches!(self, Self::Encode | Self::Decode)
    }
}

const fn declaration(
    id: PointerHelperApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(ApiId::PointerHelper(id), name, parameters, if id.encoding() { ReturnConvention::Pointer } else if matches!(id, PointerHelperApi::FlushCache) { ReturnConvention::BoolLastError } else { ReturnConvention::Uint32 },
        "existing PE32 checked-range probes and pseudo-process cache flush, zero-size probes do not touch memory, code writes already invalidate IR; signed MulDiv nearest rounding and success UINT_MAX on zero/overflow preserve LastError; pointer encoding/decoding retain architecture-width identity and PE32 note, only encoding pair present on Win64",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: if id.encoding() { &["kernel32.dll", "kernelbase.dll"] } else { &[] }, execution: if id.encoding() { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } }])
}

use Parameter::{Int32 as I, Pointer as P, Uint32 as D};
pub static POINTER_HELPER_CONTRACTS: [Contract; 7] = [
    declaration(PointerHelperApi::BadRead, "IsBadReadPtr", &[P, D]),
    declaration(PointerHelperApi::BadWrite, "IsBadWritePtr", &[P, D]),
    declaration(PointerHelperApi::BadCode, "IsBadCodePtr", &[P]),
    declaration(
        PointerHelperApi::FlushCache,
        "FlushInstructionCache",
        &[P, P, D],
    ),
    declaration(PointerHelperApi::MulDiv, "MulDiv", &[I, I, I]),
    declaration(PointerHelperApi::Encode, "EncodePointer", &[P]),
    declaration(PointerHelperApi::Decode, "DecodePointer", &[P]),
];
