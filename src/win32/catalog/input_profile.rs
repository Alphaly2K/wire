//! Existing absent-device/input-context behavior, including historical providers.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawInputApi {
    Register,
    Registered,
    DeviceList,
    Buffer,
    DeviceInfoA,
    DeviceInfoW,
    Data,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImeApi {
    GetContext,
    ReleaseContext,
    GetCompositionA,
    GetCompositionW,
    SetCompositionA,
    SetCompositionW,
    Notify,
    DefaultWindow,
    IsIme,
    GetOpen,
    SetOpen,
    Associate,
    AssociateEx,
    SetFontW,
    SetConversion,
    GetConversion,
    SetWindow,
}

const fn declaration(
    id: ApiId,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let ime = matches!(id, ApiId::Ime(_));
    Contract::new(
        id,
        name,
        parameters,
        returns,
        "PE32 known ABI-only; Win64 no raw delivery/backend or IME/HIMC; preserve empty queries/removal, HWND validation and error/output/saved-frame order; kernel providers remain static-import-only, IME retains historical user32 lookup",
        [
            ArchitecturePlan {
                dlls: if ime { &["imm32.dll"] } else { &["user32.dll"] },
                execution: ExecutionPolicy::LegacyUnimplemented,
            },
            ArchitecturePlan {
                dlls: if ime {
                    &["kernel32.dll", "kernelbase.dll", "user32.dll", "imm32.dll"]
                } else {
                    &["kernel32.dll", "kernelbase.dll", "user32.dll"]
                },
                execution: ExecutionPolicy::Typed,
            },
        ],
    )
    .with_lookup_dlls([
        if ime { &["imm32.dll"] } else { &["user32.dll"] },
        if ime { &["user32.dll", "imm32.dll"] } else { &["user32.dll"] },
    ])
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as R, Uint32 as U};
pub static RAW_INPUT_CONTRACTS: [Contract; 7] = [
    declaration(
        ApiId::RawInput(RawInputApi::Register),
        "RegisterRawInputDevices",
        &[P, D, D],
        B,
    ),
    declaration(
        ApiId::RawInput(RawInputApi::Registered),
        "GetRegisteredRawInputDevices",
        &[P, P, D],
        U,
    ),
    declaration(
        ApiId::RawInput(RawInputApi::DeviceList),
        "GetRawInputDeviceList",
        &[P, P, D],
        U,
    ),
    declaration(
        ApiId::RawInput(RawInputApi::Buffer),
        "GetRawInputBuffer",
        &[P, P, D],
        U,
    ),
    declaration(
        ApiId::RawInput(RawInputApi::DeviceInfoA),
        "GetRawInputDeviceInfoA",
        &[P, D, P, P],
        U,
    ),
    declaration(
        ApiId::RawInput(RawInputApi::DeviceInfoW),
        "GetRawInputDeviceInfoW",
        &[P, D, P, P],
        U,
    ),
    declaration(
        ApiId::RawInput(RawInputApi::Data),
        "GetRawInputData",
        &[P, D, P, P, D],
        U,
    ),
];
pub static IME_CONTRACTS: [Contract; 17] = [
    declaration(ApiId::Ime(ImeApi::GetContext), "ImmGetContext", &[P], R),
    declaration(
        ApiId::Ime(ImeApi::ReleaseContext),
        "ImmReleaseContext",
        &[P, P],
        B,
    ),
    declaration(
        ApiId::Ime(ImeApi::GetCompositionA),
        "ImmGetCompositionStringA",
        &[P, D, P, D],
        U,
    ),
    declaration(
        ApiId::Ime(ImeApi::GetCompositionW),
        "ImmGetCompositionStringW",
        &[P, D, P, D],
        U,
    ),
    declaration(
        ApiId::Ime(ImeApi::SetCompositionA),
        "ImmSetCompositionStringA",
        &[P, D, P, D, P, D],
        B,
    ),
    declaration(
        ApiId::Ime(ImeApi::SetCompositionW),
        "ImmSetCompositionStringW",
        &[P, D, P, D, P, D],
        B,
    ),
    declaration(ApiId::Ime(ImeApi::Notify), "ImmNotifyIME", &[P, D, D, D], B),
    declaration(
        ApiId::Ime(ImeApi::DefaultWindow),
        "ImmGetDefaultIMEWnd",
        &[P],
        R,
    ),
    declaration(ApiId::Ime(ImeApi::IsIme), "ImmIsIME", &[P], B),
    declaration(ApiId::Ime(ImeApi::GetOpen), "ImmGetOpenStatus", &[P], B),
    declaration(ApiId::Ime(ImeApi::SetOpen), "ImmSetOpenStatus", &[P, D], B),
    declaration(
        ApiId::Ime(ImeApi::Associate),
        "ImmAssociateContext",
        &[P, P],
        R,
    ),
    declaration(
        ApiId::Ime(ImeApi::AssociateEx),
        "ImmAssociateContextEx",
        &[P, P, D],
        B,
    ),
    declaration(
        ApiId::Ime(ImeApi::SetFontW),
        "ImmSetCompositionFontW",
        &[P, P],
        B,
    ),
    declaration(
        ApiId::Ime(ImeApi::SetConversion),
        "ImmSetConversionStatus",
        &[P, D, D],
        B,
    ),
    declaration(
        ApiId::Ime(ImeApi::GetConversion),
        "ImmGetConversionStatus",
        &[P, P, P],
        B,
    ),
    declaration(
        ApiId::Ime(ImeApi::SetWindow),
        "ImmSetCompositionWindow",
        &[P, P],
        B,
    ),
];
