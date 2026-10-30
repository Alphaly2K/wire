//! Window text and input ownership; PE32 callbacks remain explicit control.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowStateApi {
    SetTextA,
    SetTextW,
    TextA,
    TextW,
    TextLengthA,
    TextLengthW,
    Focus,
    SetFocus,
    Active,
    SetActive,
    Foreground,
    SetForeground,
    Capture,
    SetCapture,
    ReleaseCapture,
    LastPopup,
    AllowForeground,
}
impl WindowStateApi {
    pub(crate) fn wide(self) -> bool {
        matches!(self, Self::SetTextW | Self::TextW | Self::TextLengthW)
    }
    pub(crate) const fn win64(self) -> bool {
        !matches!(
            self,
            Self::SetActive
                | Self::Capture
                | Self::SetCapture
                | Self::ReleaseCapture
                | Self::LastPopup
        )
    }
}

const fn declaration(
    id: WindowStateApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    if matches!(id, WindowStateApi::AllowForeground) {
        return Contract::new(ApiId::WindowState(id), name, parameters, returns,
            "Foreground permission transfer is unavailable: FALSE/ERROR_NOT_SUPPORTED(50), no permission or foreground-state mutation.",
            [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2]);
    }
    Contract::new(
        ApiId::WindowState(id), name, parameters, returns,
        "Historical ABI plans retained below. Current adapters invoke application WNDPROC text callbacks in both ABIs after HWND/thread checks; input ownership commits before reentrant notifications. Cross-thread services and foreground permission transfer remain unavailable; saved frames and complete message words are retained.",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: if id.win64() { &["kernel32.dll", "kernelbase.dll", "user32.dll"] } else { &[] }, execution: if id.win64() { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } }],
    ).with_lookup_dlls([&["user32.dll"], if id.win64() { &["user32.dll"] } else { &[] }])
}
use Parameter::{Pointer as P, Uint32 as U};
use ReturnConvention::{BoolLastError as B, CharacterCount as C, Pointer as H};
pub static WINDOW_STATE_CONTRACTS: [Contract; 17] = [
    declaration(WindowStateApi::SetTextA, "SetWindowTextA", &[P, P], B),
    declaration(WindowStateApi::SetTextW, "SetWindowTextW", &[P, P], B),
    declaration(WindowStateApi::TextA, "GetWindowTextA", &[P, P, U], C),
    declaration(WindowStateApi::TextW, "GetWindowTextW", &[P, P, U], C),
    declaration(WindowStateApi::TextLengthA, "GetWindowTextLengthA", &[P], C),
    declaration(WindowStateApi::TextLengthW, "GetWindowTextLengthW", &[P], C),
    declaration(WindowStateApi::Focus, "GetFocus", &[], H),
    declaration(WindowStateApi::SetFocus, "SetFocus", &[P], H),
    declaration(WindowStateApi::Active, "GetActiveWindow", &[], H),
    declaration(WindowStateApi::SetActive, "SetActiveWindow", &[P], H),
    declaration(WindowStateApi::Foreground, "GetForegroundWindow", &[], H),
    declaration(
        WindowStateApi::SetForeground,
        "SetForegroundWindow",
        &[P],
        B,
    ),
    declaration(WindowStateApi::Capture, "GetCapture", &[], H),
    declaration(WindowStateApi::SetCapture, "SetCapture", &[P], H),
    declaration(WindowStateApi::ReleaseCapture, "ReleaseCapture", &[], B),
    declaration(WindowStateApi::LastPopup, "GetLastActivePopup", &[P], H),
    declaration(
        WindowStateApi::AllowForeground,
        "AllowSetForegroundWindow",
        &[U],
        B,
    ),
];
