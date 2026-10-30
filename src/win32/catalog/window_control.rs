//! Window lifecycle, positioning and synchronous procedure control boundaries.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowControlApi {
    CreateA,
    CreateW,
    Destroy,
    DefaultA,
    DefaultW,
    DefaultDialogA,
    DefaultDialogW,
    CallA,
    CallW,
    Show,
    Update,
    Enable,
    Parent,
    Move,
    Position,
    SendA,
    SendW,
    SendTimeoutA,
    SendTimeoutW,
}
impl WindowControlApi {
    pub(crate) const fn timeout(self) -> bool {
        matches!(self, Self::SendTimeoutA | Self::SendTimeoutW)
    }
    pub(crate) fn wide(self) -> bool {
        matches!(
            self,
            Self::CreateW
                | Self::DefaultW
                | Self::DefaultDialogW
                | Self::CallW
                | Self::SendW
                | Self::SendTimeoutW
        )
    }
    pub(crate) fn default_procedure(self) -> bool {
        matches!(
            self,
            Self::DefaultA | Self::DefaultW | Self::DefaultDialogA | Self::DefaultDialogW
        )
    }
    pub(crate) const fn callback(self) -> bool {
        !matches!(self, Self::Update)
    }
    const fn win64(self) -> bool {
        !matches!(
            self,
            Self::DefaultDialogA | Self::DefaultDialogW | Self::Enable | Self::Parent
        )
    }
}
const fn declaration(
    id: WindowControlApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    if id.timeout() {
        return Contract::new(
            ApiId::WindowControl(id), name, parameters, returns,
            "Same-thread WNDPROC continuation, full-width message/result and saved original frame; timeout ignored for the same queue, ERRORONEXIT detects window destruction; optional result is preflighted before callback and published after a frozen completion. Cross-thread, broadcast and cross-A/W text marshalling remain unavailable.",
            [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2],
        );
    }
    Contract::new(
        ApiId::WindowControl(id), name, parameters, returns,
        "retain original creation/default/forwarding callback, message blocking, return-frame and partial state commit rules; Win64 WNDPROC requires process executor and valid callback frame, UpdateWindow remains scalar; omitted Win64 APIs absent; static-only kernel aliases",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: if id.win64() { &["kernel32.dll", "kernelbase.dll", "user32.dll"] } else { &[] }, execution: if id.win64() { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } }],
    ).with_lookup_dlls([&["user32.dll"], if id.win64() { &["user32.dll"] } else { &[] }])
}
use Parameter::{Int32 as I, Pointer as P, Uint32 as U};
use ReturnConvention::{BoolLastError as B, Pointer as H};
pub static WINDOW_CONTROL_CONTRACTS: [Contract; 19] = [
    declaration(
        WindowControlApi::CreateA,
        "CreateWindowExA",
        &[U, P, P, U, I, I, I, I, P, P, P, P],
        H,
    ),
    declaration(
        WindowControlApi::CreateW,
        "CreateWindowExW",
        &[U, P, P, U, I, I, I, I, P, P, P, P],
        H,
    ),
    declaration(WindowControlApi::Destroy, "DestroyWindow", &[P], B),
    declaration(
        WindowControlApi::DefaultA,
        "DefWindowProcA",
        &[P, U, P, P],
        H,
    ),
    declaration(
        WindowControlApi::DefaultW,
        "DefWindowProcW",
        &[P, U, P, P],
        H,
    ),
    declaration(
        WindowControlApi::DefaultDialogA,
        "DefDlgProcA",
        &[P, U, P, P],
        H,
    ),
    declaration(
        WindowControlApi::DefaultDialogW,
        "DefDlgProcW",
        &[P, U, P, P],
        H,
    ),
    declaration(
        WindowControlApi::CallA,
        "CallWindowProcA",
        &[P, P, U, P, P],
        H,
    ),
    declaration(
        WindowControlApi::CallW,
        "CallWindowProcW",
        &[P, P, U, P, P],
        H,
    ),
    declaration(WindowControlApi::Show, "ShowWindow", &[P, I], B),
    declaration(WindowControlApi::Update, "UpdateWindow", &[P], B),
    declaration(WindowControlApi::Enable, "EnableWindow", &[P, U], B),
    declaration(WindowControlApi::Parent, "SetParent", &[P, P], H),
    declaration(WindowControlApi::Move, "MoveWindow", &[P, I, I, I, I, U], B),
    declaration(
        WindowControlApi::Position,
        "SetWindowPos",
        &[P, P, I, I, I, I, U],
        B,
    ),
    declaration(WindowControlApi::SendA, "SendMessageA", &[P, U, P, P], H),
    declaration(WindowControlApi::SendW, "SendMessageW", &[P, U, P, P], H),
    declaration(
        WindowControlApi::SendTimeoutA,
        "SendMessageTimeoutA",
        &[P, U, P, P, U, U, P],
        H,
    ),
    declaration(
        WindowControlApi::SendTimeoutW,
        "SendMessageTimeoutW",
        &[P, U, P, P, U, U, P],
        H,
    ),
];
