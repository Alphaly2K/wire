//! PE32 control wrappers, enumeration and explicit unavailable USER services.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogApi {
    GetTextW,
    GetTextA,
    SetTextW,
    SetTextA,
    SendW,
    SendA,
    Item,
    ControlId,
    GetInteger,
    SetInteger,
    Children,
    ModalIndirectW,
    CreateIndirectW,
    MapRect,
    IsMessageW,
    CreateIcon,
    Placement,
    GetScroll,
    SetScroll,
    LockUpdate,
    Windows,
    ThreadWindows,
}
impl DialogApi {
    pub(crate) const fn unavailable(self) -> bool {
        matches!(
            self,
            Self::ModalIndirectW
                | Self::CreateIndirectW
                | Self::MapRect
                | Self::IsMessageW
                | Self::CreateIcon
                | Self::Placement
                | Self::GetScroll
                | Self::SetScroll
                | Self::LockUpdate
        )
    }
    pub(crate) const fn message(self) -> bool {
        matches!(
            self,
            Self::GetTextW
                | Self::GetTextA
                | Self::SetTextW
                | Self::SetTextA
                | Self::SendW
                | Self::SendA
                | Self::GetInteger
                | Self::SetInteger
        )
    }
    pub(crate) const fn send(self) -> bool {
        matches!(self, Self::SendW | Self::SendA)
    }
    pub(crate) const fn set_text(self) -> bool {
        matches!(self, Self::SetTextW | Self::SetTextA)
    }
}
const fn declaration(
    id: DialogApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(ApiId::Dialog(id), name, parameters, returns,
        "retain PE32 child lookup/thread checks, pre-callback integer output, decimal parsing, snapshot enumeration and callback transfer; explicit unavailable services retain error 120 and unimplemented trace; Win64 exports absent",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }])
}
use Parameter::{Int32 as I, Pointer as P, Uint32 as U};
use ReturnConvention::{BoolLastError as B, Pointer as H, Uint32 as D};
pub static DIALOG_CONTRACTS: [Contract; 22] = [
    declaration(DialogApi::GetTextW, "GetDlgItemTextW", &[P, I, P, I], D),
    declaration(DialogApi::GetTextA, "GetDlgItemTextA", &[P, I, P, I], D),
    declaration(DialogApi::SetTextW, "SetDlgItemTextW", &[P, I, P], B),
    declaration(DialogApi::SetTextA, "SetDlgItemTextA", &[P, I, P], B),
    declaration(DialogApi::SendW, "SendDlgItemMessageW", &[P, I, U, P, P], H),
    declaration(DialogApi::SendA, "SendDlgItemMessageA", &[P, I, U, P, P], H),
    declaration(DialogApi::Item, "GetDlgItem", &[P, I], H),
    declaration(DialogApi::ControlId, "GetDlgCtrlID", &[P], D),
    declaration(DialogApi::GetInteger, "GetDlgItemInt", &[P, I, P, U], D),
    declaration(DialogApi::SetInteger, "SetDlgItemInt", &[P, I, U, U], B),
    declaration(DialogApi::Children, "EnumChildWindows", &[P, P, P], B),
    declaration(
        DialogApi::ModalIndirectW,
        "DialogBoxIndirectParamW",
        &[P, P, P, P, P],
        H,
    ),
    declaration(
        DialogApi::CreateIndirectW,
        "CreateDialogIndirectParamW",
        &[P, P, P, P, P],
        H,
    ),
    declaration(DialogApi::MapRect, "MapDialogRect", &[P, P], B),
    declaration(DialogApi::IsMessageW, "IsDialogMessageW", &[P, P], B),
    declaration(DialogApi::CreateIcon, "CreateIconIndirect", &[P], H),
    declaration(DialogApi::Placement, "SetWindowPlacement", &[P, P], B),
    declaration(DialogApi::GetScroll, "GetScrollInfo", &[P, I, P], B),
    declaration(DialogApi::SetScroll, "SetScrollInfo", &[P, I, P, U], D),
    declaration(DialogApi::LockUpdate, "LockWindowUpdate", &[P], B),
    declaration(DialogApi::Windows, "EnumWindows", &[P, P], B),
    declaration(DialogApi::ThreadWindows, "EnumThreadWindows", &[U, P, P], B),
];
