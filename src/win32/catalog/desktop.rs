//! Fixed input/font profile, process-local settings and limited DWM queries.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopApi {
    ParametersA,
    ParametersW,
    DoubleClickTime,
    DialogBaseUnits,
    WindowAttribute,
    CompositionEnabled,
    ProcessStation,
    ObjectInformationW,
    ObjectInformationA,
    ThreadDesktop,
    CaretBlinkTime,
}

const fn declaration(
    id: DesktopApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    if matches!(
        id,
        DesktopApi::ObjectInformationA | DesktopApi::ThreadDesktop | DesktopApi::CaretBlinkTime
    ) {
        return Contract::new(ApiId::Desktop(id), name, parameters, returns,
            "Virtual desktop profile has a fixed 530 ms caret preference; actual caret creation/rendering remains unavailable. Desktop/station handles and object queries fail with zero/120, preserving all query outputs; no synthetic handle is published.",
            [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2]);
    }
    let settings = matches!(id, DesktopApi::ParametersA | DesktopApi::ParametersW);
    let fixed = matches!(
        id,
        DesktopApi::DoubleClickTime
            | DesktopApi::DialogBaseUnits
            | DesktopApi::ProcessStation
            | DesktopApi::ObjectInformationW
    );
    let timing = matches!(id, DesktopApi::DoubleClickTime);
    Contract::new(
        ApiId::Desktop(id), name, parameters, returns,
        "Historical ABI plans retained below. Current adapters share logical desktop preferences/settings, explicit zero/120 station/desktop failures and checked DWM query subsets; no synthetic station handle, host preference mutation or compositor. Current provider admission is independent of the historical plans.",
        [ArchitecturePlan { dlls: if settings || fixed { &["user32.dll"] } else { &[] }, execution: if settings { ExecutionPolicy::LegacyUnimplemented } else if fixed { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } },
         ArchitecturePlan { dlls: if timing { &["user32.dll"] } else if fixed { &[] } else if settings { &["kernel32.dll", "kernelbase.dll", "user32.dll"] } else { &["kernel32.dll", "kernelbase.dll", "user32.dll", "dwmapi.dll"] }, execution: if fixed && !timing { ExecutionPolicy::Absent } else { ExecutionPolicy::Typed } }],
    ).with_lookup_dlls([
        if settings || fixed { &["user32.dll"] } else { &[] },
        if timing { &["user32.dll"] } else if settings || fixed { &[] } else { &["user32.dll", "dwmapi.dll"] },
    ])
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Hresult as H, Uint32 as U};
pub static DESKTOP_CONTRACTS: [Contract; 11] = [
    declaration(
        DesktopApi::ProcessStation,
        "GetProcessWindowStation",
        &[],
        ReturnConvention::Pointer,
    ),
    declaration(
        DesktopApi::ObjectInformationW,
        "GetUserObjectInformationW",
        &[P, D, P, D, P],
        B,
    ),
    declaration(
        DesktopApi::ParametersA,
        "SystemParametersInfoA",
        &[D, D, P, D],
        B,
    ),
    declaration(
        DesktopApi::ParametersW,
        "SystemParametersInfoW",
        &[D, D, P, D],
        B,
    ),
    declaration(DesktopApi::DoubleClickTime, "GetDoubleClickTime", &[], U),
    declaration(DesktopApi::DialogBaseUnits, "GetDialogBaseUnits", &[], U),
    declaration(
        DesktopApi::WindowAttribute,
        "DwmGetWindowAttribute",
        &[P, D, P, D],
        H,
    ),
    declaration(
        DesktopApi::CompositionEnabled,
        "DwmIsCompositionEnabled",
        &[P],
        H,
    ),
    declaration(
        DesktopApi::ObjectInformationA,
        "GetUserObjectInformationA",
        &[P, D, P, D, P],
        B,
    ),
    declaration(
        DesktopApi::ThreadDesktop,
        "GetThreadDesktop",
        &[D],
        ReturnConvention::Pointer,
    ),
    declaration(DesktopApi::CaretBlinkTime, "GetCaretBlinkTime", &[], U),
];
