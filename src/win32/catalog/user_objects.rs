//! Registered process names and window properties; enumeration stays ABI-only.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserObjectApi {
    ClipboardA,
    ClipboardW,
    MessageA,
    MessageW,
    SetA,
    SetW,
    GetA,
    GetW,
    RemoveA,
    RemoveW,
    EnumA,
    EnumW,
    EnumExA,
    EnumExW,
    OpenClipboard,
    CloseClipboard,
    EmptyClipboard,
    GetClipboard,
    SetClipboard,
    ClipboardAvailable,
}

impl UserObjectApi {
    pub(crate) const fn clipboard_data(self) -> bool {
        matches!(
            self,
            Self::OpenClipboard
                | Self::CloseClipboard
                | Self::EmptyClipboard
                | Self::GetClipboard
                | Self::SetClipboard
                | Self::ClipboardAvailable
        )
    }
    pub(crate) fn wide(self) -> bool {
        matches!(
            self,
            Self::ClipboardW
                | Self::MessageW
                | Self::SetW
                | Self::GetW
                | Self::RemoveW
                | Self::EnumW
                | Self::EnumExW
        )
    }
    pub(crate) fn registered(self) -> bool {
        matches!(
            self,
            Self::ClipboardA | Self::ClipboardW | Self::MessageA | Self::MessageW
        )
    }
    pub(crate) const fn enumeration(self) -> bool {
        matches!(
            self,
            Self::EnumA | Self::EnumW | Self::EnumExA | Self::EnumExW
        )
    }
}

const fn declaration(
    id: UserObjectApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    if id.clipboard_data() {
        return Contract::new(ApiId::UserObject(id), name, parameters, returns,
            "No clipboard backend or data owner: OpenClipboard fails 50 after HWND validation, close/empty/get/set fail ERROR_CLIPBOARD_NOT_OPEN(1418); no format is available, and availability queries preserve LastError. No handle transfer, data access or delayed-render callback.",
            [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2]);
    }
    Contract::new(
        ApiId::UserObject(id), name, parameters, returns,
        "Historical ABI plans retained below. Current adapters share process-only registered format/message names, Unicode lowercase identity, 255 UTF-16-unit limit, monotonic 0xffff capacity and per-window properties across both ABIs. Atom/name keys, input-before-HWND checks, error/return order and unsupported property enumeration are retained; clipboard data is a separate unavailable backend.",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: if id.enumeration() { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Typed } },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }],
    ).with_lookup_dlls([&["user32.dll"], &[]])
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as H, Uint32 as U};
pub static USER_OBJECT_CONTRACTS: [Contract; 20] = [
    declaration(
        UserObjectApi::ClipboardA,
        "RegisterClipboardFormatA",
        &[P],
        U,
    ),
    declaration(
        UserObjectApi::ClipboardW,
        "RegisterClipboardFormatW",
        &[P],
        U,
    ),
    declaration(UserObjectApi::MessageA, "RegisterWindowMessageA", &[P], U),
    declaration(UserObjectApi::MessageW, "RegisterWindowMessageW", &[P], U),
    declaration(UserObjectApi::SetA, "SetPropA", &[P, P, P], B),
    declaration(UserObjectApi::SetW, "SetPropW", &[P, P, P], B),
    declaration(UserObjectApi::GetA, "GetPropA", &[P, P], H),
    declaration(UserObjectApi::GetW, "GetPropW", &[P, P], H),
    declaration(UserObjectApi::RemoveA, "RemovePropA", &[P, P], H),
    declaration(UserObjectApi::RemoveW, "RemovePropW", &[P, P], H),
    declaration(UserObjectApi::EnumA, "EnumPropsA", &[P, P], U),
    declaration(UserObjectApi::EnumW, "EnumPropsW", &[P, P], U),
    declaration(UserObjectApi::EnumExA, "EnumPropsExA", &[P, P, P], U),
    declaration(UserObjectApi::EnumExW, "EnumPropsExW", &[P, P, P], U),
    declaration(UserObjectApi::OpenClipboard, "OpenClipboard", &[P], B),
    declaration(UserObjectApi::CloseClipboard, "CloseClipboard", &[], B),
    declaration(UserObjectApi::EmptyClipboard, "EmptyClipboard", &[], B),
    declaration(UserObjectApi::GetClipboard, "GetClipboardData", &[D], H),
    declaration(UserObjectApi::SetClipboard, "SetClipboardData", &[D, P], H),
    declaration(
        UserObjectApi::ClipboardAvailable,
        "IsClipboardFormatAvailable",
        &[D],
        B,
    ),
];
