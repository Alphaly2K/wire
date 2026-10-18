//! The legacy menu services are absent from the Win64 runtime.
#[test]
fn menu_contract_win64_keeps_menu_and_tracking_exports_absent() {
    for dll in ["user32.dll", "kernel32.dll", "kernelbase.dll"] {
        for name in [
            "CreateMenu",
            "CreatePopupMenu",
            "DestroyMenu",
            "IsMenu",
            "GetMenu",
            "SetMenu",
            "GetMenuItemCount",
            "InsertMenuItemA",
            "InsertMenuItemW",
            "SetMenuItemInfoA",
            "SetMenuItemInfoW",
            "GetMenuItemInfoA",
            "GetMenuItemInfoW",
            "RemoveMenu",
            "DeleteMenu",
            "GetSubMenu",
            "GetMenuItemID",
            "GetMenuState",
            "EnableMenuItem",
            "CheckMenuItem",
            "DrawMenuBar",
            "TrackPopupMenu",
            "TrackPopupMenuEx",
            "RedrawWindow",
            "LoadMenuA",
            "LoadMenuW",
            "GetSystemMenu",
        ] {
            assert!(
                !wire::x64::runtime::import_supported(dll, name),
                "{dll}!{name}"
            );
        }
    }
}
