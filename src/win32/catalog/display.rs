//! Virtual display queries, packed POINT and explicit monitor callback control.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayApi {
    DevicesA,
    DevicesW,
    SettingsA,
    SettingsW,
    SettingsExA,
    SettingsExW,
    Monitors,
    FromPoint,
    FromRect,
    FromWindow,
    InfoA,
    InfoW,
    ConfigSizes,
    ConfigQuery,
    ConfigDeviceInfo,
    Metrics,
}

impl DisplayApi {
    pub(crate) fn wide(self) -> bool {
        matches!(
            self,
            Self::DevicesW | Self::SettingsW | Self::SettingsExW | Self::InfoW
        )
    }
    pub(crate) fn devices(self) -> bool {
        matches!(self, Self::DevicesA | Self::DevicesW)
    }
    pub(crate) fn settings(self) -> bool {
        matches!(
            self,
            Self::SettingsA | Self::SettingsW | Self::SettingsExA | Self::SettingsExW
        )
    }
    pub(crate) fn extended(self) -> bool {
        matches!(self, Self::SettingsExA | Self::SettingsExW)
    }
}

const fn declaration(
    id: DisplayApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let ccd = matches!(
        id,
        DisplayApi::ConfigSizes | DisplayApi::ConfigQuery | DisplayApi::ConfigDeviceInfo
    );
    let metrics = matches!(id, DisplayApi::Metrics);
    Contract::new(
        ApiId::Display(id), name, parameters, returns,
        "one virtual 1024x768/32-bit/60-Hz desktop; keep PE32 broad monitor lookup, adapter-only enumeration, copy-fault error mapping and CCD ABI-only policy; Win64 packed POINT/intersection/HWND validation, adapter/monitor identity and direct unsupported CCD status; monitor callbacks remain explicit process control with original resources/frame/error/output order; kernel providers remain static-import-only and Win64 metrics retains its dynamic lookup gap with existing-gateway fallback",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: if ccd { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Typed } },
         ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll", "user32.dll"], execution: ExecutionPolicy::Typed }],
    ).with_lookup_dlls([&["user32.dll"], if metrics { &[] } else { &["user32.dll"] }])
}

use Parameter::{Int32 as I, Pointer as P, Uint32 as D, Uint64 as Q};
use ReturnConvention::{BoolLastError as B, Pointer as R, Uint32 as U};
pub static DISPLAY_CONTRACTS: [Contract; 16] = [
    declaration(
        DisplayApi::DevicesA,
        "EnumDisplayDevicesA",
        &[P, D, P, D],
        B,
    ),
    declaration(
        DisplayApi::DevicesW,
        "EnumDisplayDevicesW",
        &[P, D, P, D],
        B,
    ),
    declaration(DisplayApi::SettingsA, "EnumDisplaySettingsA", &[P, D, P], B),
    declaration(DisplayApi::SettingsW, "EnumDisplaySettingsW", &[P, D, P], B),
    declaration(
        DisplayApi::SettingsExA,
        "EnumDisplaySettingsExA",
        &[P, D, P, D],
        B,
    ),
    declaration(
        DisplayApi::SettingsExW,
        "EnumDisplaySettingsExW",
        &[P, D, P, D],
        B,
    ),
    declaration(
        DisplayApi::Monitors,
        "EnumDisplayMonitors",
        &[P, P, P, P],
        B,
    ),
    declaration(DisplayApi::FromPoint, "MonitorFromPoint", &[Q, D], R),
    declaration(DisplayApi::FromRect, "MonitorFromRect", &[P, D], R),
    declaration(DisplayApi::FromWindow, "MonitorFromWindow", &[P, D], R),
    declaration(DisplayApi::InfoA, "GetMonitorInfoA", &[P, P], B),
    declaration(DisplayApi::InfoW, "GetMonitorInfoW", &[P, P], B),
    declaration(
        DisplayApi::ConfigSizes,
        "GetDisplayConfigBufferSizes",
        &[D, P, P],
        U,
    ),
    declaration(
        DisplayApi::ConfigQuery,
        "QueryDisplayConfig",
        &[D, P, P, P, P, P],
        U,
    ),
    declaration(
        DisplayApi::ConfigDeviceInfo,
        "DisplayConfigGetDeviceInfo",
        &[P],
        U,
    ),
    declaration(DisplayApi::Metrics, "GetSystemMetrics", &[I], U),
];
