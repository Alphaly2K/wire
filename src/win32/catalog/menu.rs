//! Process-owned PE32 menus and explicit legacy drawing/loading policies.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuApi {
    Create,
    CreatePopup,
    Destroy,
    IsMenu,
    Get,
    Set,
    ItemCount,
    InsertA,
    InsertW,
    SetItemA,
    SetItemW,
    GetItemA,
    GetItemW,
    Remove,
    Delete,
    Submenu,
    ItemId,
    State,
    Enable,
    Check,
    DrawBar,
    Track,
    TrackEx,
    Redraw,
    LoadA,
    LoadW,
    System,
}
impl MenuApi {
    pub(crate) const fn wide(self) -> bool {
        matches!(self, Self::InsertW | Self::SetItemW | Self::GetItemW)
    }
    pub(crate) const fn inserting(self) -> bool {
        matches!(self, Self::InsertA | Self::InsertW)
    }
    pub(crate) const fn unavailable(self) -> bool {
        matches!(
            self,
            Self::DrawBar
                | Self::Track
                | Self::TrackEx
                | Self::Redraw
                | Self::LoadA
                | Self::LoadW
                | Self::System
        )
    }
    pub(crate) const fn minus_one_on_error(self) -> bool {
        matches!(
            self,
            Self::ItemCount | Self::ItemId | Self::State | Self::Enable | Self::Check
        )
    }
}
const fn declaration(
    id: MenuApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Menu(id), name, parameters, returns,
        "retain PE32 menu ownership, shared submenu retirement, child ID distinction, MENUITEMINFO validation/output prechecks and rollback; drawing/tracking including gateway RedrawWindow retain error 120 and menu note; loading/system menu remain ABI-only with empty note; Win64 absent",
        [ArchitecturePlan {
            dlls: &["user32.dll"],
            execution: if matches!(id, MenuApi::LoadA | MenuApi::LoadW | MenuApi::System) {
                ExecutionPolicy::LegacyUnimplemented
            } else {
                ExecutionPolicy::Typed
            },
        }, ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }],
    )
}
use Parameter::{Int32 as I, Pointer as P, Uint32 as U};
use ReturnConvention::{BoolLastError as B, Pointer as H, Uint32 as D};
pub static MENU_CONTRACTS: [Contract; 27] = [
    declaration(MenuApi::Create, "CreateMenu", &[], H),
    declaration(MenuApi::CreatePopup, "CreatePopupMenu", &[], H),
    declaration(MenuApi::Destroy, "DestroyMenu", &[P], B),
    declaration(MenuApi::IsMenu, "IsMenu", &[P], B),
    declaration(MenuApi::Get, "GetMenu", &[P], H),
    declaration(MenuApi::Set, "SetMenu", &[P, P], B),
    declaration(MenuApi::ItemCount, "GetMenuItemCount", &[P], D),
    declaration(MenuApi::InsertA, "InsertMenuItemA", &[P, U, U, P], B),
    declaration(MenuApi::InsertW, "InsertMenuItemW", &[P, U, U, P], B),
    declaration(MenuApi::SetItemA, "SetMenuItemInfoA", &[P, U, U, P], B),
    declaration(MenuApi::SetItemW, "SetMenuItemInfoW", &[P, U, U, P], B),
    declaration(MenuApi::GetItemA, "GetMenuItemInfoA", &[P, U, U, P], B),
    declaration(MenuApi::GetItemW, "GetMenuItemInfoW", &[P, U, U, P], B),
    declaration(MenuApi::Remove, "RemoveMenu", &[P, U, U], B),
    declaration(MenuApi::Delete, "DeleteMenu", &[P, U, U], B),
    declaration(MenuApi::Submenu, "GetSubMenu", &[P, I], H),
    declaration(MenuApi::ItemId, "GetMenuItemID", &[P, I], D),
    declaration(MenuApi::State, "GetMenuState", &[P, U, U], D),
    declaration(MenuApi::Enable, "EnableMenuItem", &[P, U, U], D),
    declaration(MenuApi::Check, "CheckMenuItem", &[P, U, U], D),
    declaration(MenuApi::DrawBar, "DrawMenuBar", &[P], B),
    declaration(MenuApi::Track, "TrackPopupMenu", &[P, U, I, I, I, P, P], B),
    declaration(MenuApi::TrackEx, "TrackPopupMenuEx", &[P, U, I, I, P, P], B),
    declaration(MenuApi::Redraw, "RedrawWindow", &[P, P, P, U], B),
    declaration(MenuApi::LoadA, "LoadMenuA", &[P, P], H),
    declaration(MenuApi::LoadW, "LoadMenuW", &[P, P], H),
    declaration(MenuApi::System, "GetSystemMenu", &[P, U], H),
];
