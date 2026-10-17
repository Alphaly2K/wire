use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinrtApi {
    CreateString,
    CreateReference,
    DeleteString,
    DuplicateString,
    StringLength,
    IsEmpty,
    RawBuffer,
    HasEmbeddedNull,
    CompareOrdinal,
    ActivationFactory,
    ActivateInstance,
}

const fn declaration(
    id: WinrtApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let activation = matches!(id, WinrtApi::ActivationFactory | WinrtApi::ActivateInstance);
    Contract::new(
        ApiId::Winrt(id), name, parameters, returns,
        "checked Win64 HSTRING subset: caller-owned reference headers reread on access, immutable owned copies with counted references, bounded UTF-16 code units, original output and saved-frame fault order, preserved LastError and activation lookup failure without output mutation; PE32 unknown ABI remains absent; dynamic WinRT API sets retain existing combase remapping",
        [
            ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent },
            ArchitecturePlan {
                dlls: if activation {
                    &["combase.dll", "api-ms-win-core-winrt-l1-1-0.dll"]
                } else {
                    &["combase.dll", "api-ms-win-core-winrt-string-l1-1-0.dll"]
                },
                execution: ExecutionPolicy::Typed,
            },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{Hresult as H, Pointer as R, Uint32 as U};
pub static WINRT_CONTRACTS: [Contract; 11] = [
    declaration(WinrtApi::CreateString, "WindowsCreateString", &[P, D, P], H),
    declaration(
        WinrtApi::CreateReference,
        "WindowsCreateStringReference",
        &[P, D, P, P],
        H,
    ),
    declaration(WinrtApi::DeleteString, "WindowsDeleteString", &[P], H),
    declaration(
        WinrtApi::DuplicateString,
        "WindowsDuplicateString",
        &[P, P],
        H,
    ),
    declaration(WinrtApi::StringLength, "WindowsGetStringLen", &[P], U),
    declaration(WinrtApi::IsEmpty, "WindowsIsStringEmpty", &[P], U),
    declaration(WinrtApi::RawBuffer, "WindowsGetStringRawBuffer", &[P, P], R),
    declaration(
        WinrtApi::HasEmbeddedNull,
        "WindowsStringHasEmbeddedNull",
        &[P, P],
        H,
    ),
    declaration(
        WinrtApi::CompareOrdinal,
        "WindowsCompareStringOrdinal",
        &[P, P, P],
        H,
    ),
    declaration(
        WinrtApi::ActivationFactory,
        "RoGetActivationFactory",
        &[P, P, P],
        H,
    ),
    declaration(WinrtApi::ActivateInstance, "RoActivateInstance", &[P, P], H),
];
