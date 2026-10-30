//! Preserve the empty PE32 environment and guest-owned Win64 variables.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentApi {
    Block,
    BlockA,
    BlockW,
    FreeA,
    FreeW,
    GetA,
    GetW,
    SetA,
    SetW,
    ExpandA,
    ExpandW,
}
impl EnvironmentApi {
    pub(crate) fn scratch(self) -> bool {
        matches!(self, Self::Block | Self::BlockW)
    }
    pub(crate) fn wide(self) -> bool {
        matches!(self, Self::GetW | Self::SetW | Self::ExpandW)
    }
    pub(crate) fn set(self) -> bool {
        matches!(self, Self::SetA | Self::SetW)
    }
}
const fn declaration(
    id: EnvironmentApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let added = matches!(id, EnvironmentApi::ExpandA | EnvironmentApi::ExpandW);
    let x86_abi = matches!(
        id,
        EnvironmentApi::BlockA | EnvironmentApi::SetA | EnvironmentApi::SetW
    );
    let x64_absent = matches!(
        id,
        EnvironmentApi::Block
            | EnvironmentApi::BlockA
            | EnvironmentApi::FreeA
            | EnvironmentApi::ExpandA
            | EnvironmentApi::ExpandW
    );
    Contract::new(
        ApiId::Environment(id),
        name,
        parameters,
        returns,
        if added {
            "A/W environment expansion with pointer inputs, DWORD capacity and NUL-inclusive character count; runtime admission is separate from the absent legacy execution plans"
        } else {
            "PE32 empty variables ignore guest pointers and preserve LastError, empty blocks use the existing scratch allocator, free accepts any pointer without reclaim, ANSI-suffixed block/setters remain ABI-only; Win64 case-insensitive guest variables with existing codepage/length/error ordering, DWORD capacity and checked writes, owned wide blocks use separate page allocations and reject normal HeapFree; preserve error/CPU/frame/API-count submission and absent ANSI block exports"
        },
        [
            ArchitecturePlan {
                dlls: if added { &[] } else { &["kernel32.dll"] },
                execution: if added {
                    ExecutionPolicy::Absent
                } else if x86_abi {
                    ExecutionPolicy::LegacyUnimplemented
                } else {
                    ExecutionPolicy::Typed
                },
            },
            ArchitecturePlan {
                dlls: if x64_absent {
                    &[]
                } else {
                    &["kernel32.dll", "kernelbase.dll"]
                },
                execution: if x64_absent {
                    ExecutionPolicy::Absent
                } else {
                    ExecutionPolicy::Typed
                },
            },
        ],
    )
}
use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, CharacterCount as C, Pointer as H};
pub static ENVIRONMENT_CONTRACTS: [Contract; 11] = [
    declaration(EnvironmentApi::Block, "GetEnvironmentStrings", &[], H),
    declaration(EnvironmentApi::BlockA, "GetEnvironmentStringsA", &[], H),
    declaration(EnvironmentApi::BlockW, "GetEnvironmentStringsW", &[], H),
    declaration(EnvironmentApi::FreeA, "FreeEnvironmentStringsA", &[P], B),
    declaration(EnvironmentApi::FreeW, "FreeEnvironmentStringsW", &[P], B),
    declaration(
        EnvironmentApi::GetA,
        "GetEnvironmentVariableA",
        &[P, P, D],
        C,
    ),
    declaration(
        EnvironmentApi::GetW,
        "GetEnvironmentVariableW",
        &[P, P, D],
        C,
    ),
    declaration(EnvironmentApi::SetA, "SetEnvironmentVariableA", &[P, P], B),
    declaration(EnvironmentApi::SetW, "SetEnvironmentVariableW", &[P, P], B),
    declaration(
        EnvironmentApi::ExpandA,
        "ExpandEnvironmentStringsA",
        &[P, P, D],
        C,
    ),
    declaration(
        EnvironmentApi::ExpandW,
        "ExpandEnvironmentStringsW",
        &[P, P, D],
        C,
    ),
];
