use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariantApi {
    Initialize,
    Clear,
    Copy,
    ChangeType,
}

const fn declaration(
    id: VariantApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Variant(id), name, parameters, returns,
        "existing PE32 16-byte Variant subset: scalar byte copy, BSTR deep copy and rollback through shared owner, BYREF clear without release, clear tag only, HRESULT also stored in LastError, flags ignored and target type consumed as low WORD, original pointer/write/return order; Win64 absent, PE32 ordinal resolution unchanged",
        [
            ArchitecturePlan { dlls: &["oleaut32.dll"], execution: ExecutionPolicy::Typed },
            ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{Hresult as H, Void as V};
pub static VARIANT_CONTRACTS: [Contract; 4] = [
    declaration(VariantApi::Initialize, "VariantInit", &[P], V),
    declaration(VariantApi::Clear, "VariantClear", &[P], H),
    declaration(VariantApi::Copy, "VariantCopy", &[P, P], H),
    declaration(
        VariantApi::ChangeType,
        "VariantChangeType",
        &[P, P, D, D],
        H,
    ),
];
