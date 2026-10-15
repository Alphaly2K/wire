use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleApi {
    GetStdHandle,
    SetStdHandle,
    CloseHandle,
    DuplicateHandle,
    GetHandleInformation,
    SetHandleInformation,
}

const fn declaration(
    id: HandleApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
    pe32_typed: bool,
) -> Contract {
    Contract::new(
        ApiId::Handle(id),
        name,
        parameters,
        returns,
        "existing per-resource handle ownership, flags and duplication subsets; PE32 null standard handles and close-before-return-frame-fault; Win64 full-width handles, DWORD selectors and return-frame preflight; preserve resource-specific errors and early completion/unblock order",
        [
            ArchitecturePlan {
                dlls: &["kernel32.dll"],
                execution: if pe32_typed {
                    ExecutionPolicy::Typed
                } else {
                    ExecutionPolicy::LegacyUnimplemented
                },
            },
            ArchitecturePlan {
                dlls: &["kernel32.dll", "kernelbase.dll"],
                execution: ExecutionPolicy::Typed,
            },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as R};

pub static HANDLE_CONTRACTS: [Contract; 6] = [
    declaration(HandleApi::GetStdHandle, "GetStdHandle", &[D], R, true),
    declaration(HandleApi::SetStdHandle, "SetStdHandle", &[D, P], B, false),
    declaration(HandleApi::CloseHandle, "CloseHandle", &[P], B, true),
    declaration(
        HandleApi::DuplicateHandle,
        "DuplicateHandle",
        &[P, P, P, P, D, D, D],
        B,
        false,
    ),
    declaration(
        HandleApi::GetHandleInformation,
        "GetHandleInformation",
        &[P, P],
        B,
        false,
    ),
    declaration(
        HandleApi::SetHandleInformation,
        "SetHandleInformation",
        &[P, D, D],
        B,
        false,
    ),
];
