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
    MessageBoxA,
    MessageBoxW,
    ModalA,
    ModalW,
    ModalIndirectA,
    CreateA,
    CreateW,
    CreateIndirectA,
    End,
}
impl DialogApi {
    pub(crate) const fn modal(self) -> bool {
        matches!(
            self,
            Self::ModalA | Self::ModalW | Self::ModalIndirectA | Self::ModalIndirectW
        )
    }
    pub(crate) const fn message_box(self) -> bool {
        matches!(self, Self::MessageBoxA | Self::MessageBoxW)
    }
    pub(crate) const fn unavailable(self) -> bool {
        matches!(
            self,
            Self::ModalIndirectW
                | Self::ModalA
                | Self::ModalW
                | Self::ModalIndirectA
                | Self::CreateA
                | Self::CreateW
                | Self::CreateIndirectA
                | Self::End
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
    if matches!(
        id,
        DialogApi::ModalA
            | DialogApi::ModalW
            | DialogApi::ModalIndirectA
            | DialogApi::CreateA
            | DialogApi::CreateW
            | DialogApi::CreateIndirectA
            | DialogApi::End
    ) {
        return Contract::new(ApiId::Dialog(id), name, parameters, returns,
            "No modal dialog manager, template creation or termination: ERROR_CALL_NOT_IMPLEMENTED(120), no template/callback dereference or state mutation; modal creation returns pointer-width -1, modeless creation and EndDialog return zero.",
            [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2]);
    }
    Contract::new(ApiId::Dialog(id), name, parameters, returns,
        "Historical ABI plans retained below. Current adapters share child lookup, checked message callbacks, integer output/parsing and snapshot enumeration across both ABIs; unavailable modal creation returns -1/120, other unsupported calls return zero/120, and MessageBox has no host UI or fixed-IDOK success.",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }])
}
use Parameter::{Int32 as I, Pointer as P, Uint32 as U};
use ReturnConvention::{BoolLastError as B, Pointer as H, Uint32 as D};
pub static DIALOG_CONTRACTS: [Contract; 31] = [
    declaration(DialogApi::MessageBoxA, "MessageBoxA", &[P, P, P, U], D),
    declaration(DialogApi::MessageBoxW, "MessageBoxW", &[P, P, P, U], D),
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
    declaration(DialogApi::ModalA, "DialogBoxParamA", &[P, P, P, P, P], H),
    declaration(DialogApi::ModalW, "DialogBoxParamW", &[P, P, P, P, P], H),
    declaration(
        DialogApi::ModalIndirectA,
        "DialogBoxIndirectParamA",
        &[P, P, P, P, P],
        H,
    ),
    declaration(
        DialogApi::CreateA,
        "CreateDialogParamA",
        &[P, P, P, P, P],
        H,
    ),
    declaration(
        DialogApi::CreateW,
        "CreateDialogParamW",
        &[P, P, P, P, P],
        H,
    ),
    declaration(
        DialogApi::CreateIndirectA,
        "CreateDialogIndirectParamA",
        &[P, P, P, P, P],
        H,
    ),
    declaration(DialogApi::End, "EndDialog", &[P, P], B),
];
