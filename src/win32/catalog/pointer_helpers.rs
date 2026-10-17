use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerHelperApi {
    BadRead,
    BadWrite,
    BadCode,
    FlushCache,
    MulDiv,
}

const fn declaration(
    id: PointerHelperApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(ApiId::PointerHelper(id), name, parameters, if matches!(id, PointerHelperApi::FlushCache) { ReturnConvention::BoolLastError } else { ReturnConvention::Uint32 },
        "existing PE32 checked-range probes and pseudo-process cache flush, zero-size probes do not touch memory, code writes already invalidate IR; signed MulDiv nearest rounding and success UINT_MAX on zero/overflow preserve LastError; Win64 absent",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }])
}

use Parameter::{Int32 as I, Pointer as P, Uint32 as D};
pub static POINTER_HELPER_CONTRACTS: [Contract; 5] = [
    declaration(PointerHelperApi::BadRead, "IsBadReadPtr", &[P, D]),
    declaration(PointerHelperApi::BadWrite, "IsBadWritePtr", &[P, D]),
    declaration(PointerHelperApi::BadCode, "IsBadCodePtr", &[P]),
    declaration(
        PointerHelperApi::FlushCache,
        "FlushInstructionCache",
        &[P, P, D],
    ),
    declaration(PointerHelperApi::MulDiv, "MulDiv", &[I, I, I]),
];
