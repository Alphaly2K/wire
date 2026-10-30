//! Scalar window queries and fields. Callback operations remain separate.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowQueryApi {
    IsWindow,
    Visible,
    Unicode,
    Parent,
    ThreadProcess,
    ClientRect,
    WindowRect,
    LongA,
    LongW,
    SetLongA,
    SetLongW,
    LongPtrA,
    LongPtrW,
    SetLongPtrA,
    SetLongPtrW,
    FindA,
    FindW,
    Desktop,
    Placement,
    Iconic,
    Zoomed,
    Enabled,
    ClientToScreen,
    ScreenToClient,
    MapPoints,
    Related,
}
impl WindowQueryApi {
    pub(crate) const fn pointer(self) -> bool {
        matches!(
            self,
            Self::LongPtrA | Self::LongPtrW | Self::SetLongPtrA | Self::SetLongPtrW
        )
    }
    pub(crate) fn setting(self) -> bool {
        matches!(
            self,
            Self::SetLongA | Self::SetLongW | Self::SetLongPtrA | Self::SetLongPtrW
        )
    }
    pub(crate) const fn win64(self) -> bool {
        !matches!(
            self,
            Self::FindA
                | Self::FindW
                | Self::Desktop
                | Self::Zoomed
                | Self::Enabled
                | Self::MapPoints
        )
    }
}

const fn declaration(
    id: WindowQueryApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::WindowQuery(id), name, parameters, returns,
        "preserve architecture-specific invalid-HWND LastError, DWORD/pointer and extra-storage widths, WNDPROC validation order, geometry output fault commits and coordinate error mapping; PE32 placement remains explicit error120 with legacy trace, PE32 Ptr exports unknown ABI, Win64 omitted queries absent; kernel aliases static-only",
        [ArchitecturePlan { dlls: if id.pointer() { &[] } else { &["user32.dll"] }, execution: if id.pointer() { ExecutionPolicy::Absent } else { ExecutionPolicy::Typed } },
         ArchitecturePlan { dlls: if id.win64() { &["kernel32.dll", "kernelbase.dll", "user32.dll"] } else { &[] }, execution: if id.win64() { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } }],
    ).with_lookup_dlls([if id.pointer() { &[] } else { &["user32.dll"] }, if id.win64() { &["user32.dll"] } else { &[] }])
}
use Parameter::{Int32 as I, Pointer as P, Uint32 as U};
use ReturnConvention::{BoolLastError as B, Pointer as H, Uint32 as D};
pub static WINDOW_QUERY_CONTRACTS: [Contract; 26] = [
    Contract::new(ApiId::WindowQuery(WindowQueryApi::Related),"GetWindow",&[P,U],H,"Existing logical parent/owner and creation-order sibling hierarchy; no native z-order or enabled-popup monitor",[ArchitecturePlan {dlls:&["user32.dll"],execution:ExecutionPolicy::LegacyUnimplemented};2]),
    declaration(WindowQueryApi::IsWindow, "IsWindow", &[P], B),
    declaration(WindowQueryApi::Visible, "IsWindowVisible", &[P], B),
    declaration(WindowQueryApi::Unicode, "IsWindowUnicode", &[P], B),
    declaration(WindowQueryApi::Parent, "GetParent", &[P], H),
    declaration(
        WindowQueryApi::ThreadProcess,
        "GetWindowThreadProcessId",
        &[P, P],
        D,
    ),
    declaration(WindowQueryApi::ClientRect, "GetClientRect", &[P, P], B),
    declaration(WindowQueryApi::WindowRect, "GetWindowRect", &[P, P], B),
    declaration(WindowQueryApi::LongA, "GetWindowLongA", &[P, I], D),
    declaration(WindowQueryApi::LongW, "GetWindowLongW", &[P, I], D),
    declaration(WindowQueryApi::SetLongA, "SetWindowLongA", &[P, I, U], D),
    declaration(WindowQueryApi::SetLongW, "SetWindowLongW", &[P, I, U], D),
    declaration(WindowQueryApi::LongPtrA, "GetWindowLongPtrA", &[P, I], H),
    declaration(WindowQueryApi::LongPtrW, "GetWindowLongPtrW", &[P, I], H),
    declaration(
        WindowQueryApi::SetLongPtrA,
        "SetWindowLongPtrA",
        &[P, I, P],
        H,
    ),
    declaration(
        WindowQueryApi::SetLongPtrW,
        "SetWindowLongPtrW",
        &[P, I, P],
        H,
    ),
    declaration(WindowQueryApi::FindA, "FindWindowA", &[P, P], H),
    declaration(WindowQueryApi::FindW, "FindWindowW", &[P, P], H),
    declaration(WindowQueryApi::Desktop, "GetDesktopWindow", &[], H),
    declaration(WindowQueryApi::Placement, "GetWindowPlacement", &[P, P], B),
    declaration(WindowQueryApi::Iconic, "IsIconic", &[P], B),
    declaration(WindowQueryApi::Zoomed, "IsZoomed", &[P], B),
    declaration(WindowQueryApi::Enabled, "IsWindowEnabled", &[P], B),
    declaration(WindowQueryApi::ClientToScreen, "ClientToScreen", &[P, P], B),
    declaration(WindowQueryApi::ScreenToClient, "ScreenToClient", &[P, P], B),
    declaration(
        WindowQueryApi::MapPoints,
        "MapWindowPoints",
        &[P, P, P, U],
        D,
    ),
];
