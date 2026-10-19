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
}

const fn declaration(
    id: DesktopApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let settings = matches!(id, DesktopApi::ParametersA | DesktopApi::ParametersW);
    let fixed = matches!(
        id,
        DesktopApi::DoubleClickTime
            | DesktopApi::DialogBaseUnits
            | DesktopApi::ProcessStation
            | DesktopApi::ObjectInformationW
    );
    Contract::new(
        ApiId::Desktop(id), name, parameters, returns,
        "process-local logical desktop only; keep PE32 fixed values, synthetic process station handle and unsupported station query preserving outputs/LastError, settings ABI-only and DWM unknown ABI/zero cleanup; Win64 fixed/station queries absent, screen-reader get/set subset, no host preference mutation and no compositor; HRESULT/LastError/output/return-frame order retained; static provider aliases and default settings dynamic lookup gap retained with existing-gateway fallback",
        [ArchitecturePlan { dlls: if settings || fixed { &["user32.dll"] } else { &[] }, execution: if settings { ExecutionPolicy::LegacyUnimplemented } else if fixed { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } },
         ArchitecturePlan { dlls: if fixed { &[] } else if settings { &["kernel32.dll", "kernelbase.dll", "user32.dll"] } else { &["kernel32.dll", "kernelbase.dll", "user32.dll", "dwmapi.dll"] }, execution: if fixed { ExecutionPolicy::Absent } else { ExecutionPolicy::Typed } }],
    ).with_lookup_dlls([
        if settings || fixed { &["user32.dll"] } else { &[] },
        if settings || fixed { &[] } else { &["user32.dll", "dwmapi.dll"] },
    ])
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Hresult as H, Uint32 as U};
pub static DESKTOP_CONTRACTS: [Contract; 8] = [
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
];
