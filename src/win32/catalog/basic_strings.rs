use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasicStringApi {
    CompareOrdinal,
    CompareA,
    CompareW,
    CompareInsensitiveA,
    CompareInsensitiveW,
    Length,
    LengthA,
    LengthW,
    CopyA,
    CopyW,
    ConcatA,
    ConcatW,
    CopyCountA,
    CopyCountW,
}

const fn declaration(
    id: BasicStringApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(ApiId::BasicString(id), name, parameters, returns,
        "existing PE32 raw byte/UTF-16 copy subset, 1M-unit read cap, signed bounded count, read-before-write aliases, single-unit Unicode uppercase mapping, original output/error order and LastError; unqualified lstrlen retains its original 4-byte stdcall ABI; Win64 absent",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }])
}

use Parameter::{Int32 as I, Pointer as P, Uint32 as D};
use ReturnConvention::{CharacterCount as C, Pointer as R, Uint32 as U};
pub static BASIC_STRING_CONTRACTS: [Contract; 14] = [
    declaration(
        BasicStringApi::CompareOrdinal,
        "CompareStringOrdinal",
        &[P, I, P, I, D],
        U,
    ),
    declaration(BasicStringApi::CompareA, "lstrcmpA", &[P, P], U),
    declaration(BasicStringApi::CompareW, "lstrcmpW", &[P, P], U),
    declaration(BasicStringApi::CompareInsensitiveA, "lstrcmpiA", &[P, P], U),
    declaration(BasicStringApi::CompareInsensitiveW, "lstrcmpiW", &[P, P], U),
    declaration(BasicStringApi::LengthA, "lstrlenA", &[P], C),
    declaration(BasicStringApi::Length, "lstrlen", &[P], C),
    declaration(BasicStringApi::LengthW, "lstrlenW", &[P], C),
    declaration(BasicStringApi::CopyA, "lstrcpyA", &[P, P], R),
    declaration(BasicStringApi::CopyW, "lstrcpyW", &[P, P], R),
    declaration(BasicStringApi::ConcatA, "lstrcatA", &[P, P], R),
    declaration(BasicStringApi::ConcatW, "lstrcatW", &[P, P], R),
    declaration(BasicStringApi::CopyCountA, "lstrcpynA", &[P, P, I], R),
    declaration(BasicStringApi::CopyCountW, "lstrcpynW", &[P, P, I], R),
];
