//! The original dialog/control and enumeration service surface is PE32 only.
#[test]
fn dialog_contract_win64_keeps_control_and_enumeration_exports_absent() {
    for dll in ["user32.dll", "kernel32.dll", "kernelbase.dll"] {
        for name in [
            "GetDlgItemTextW",
            "GetDlgItemTextA",
            "SetDlgItemTextW",
            "SetDlgItemTextA",
            "SendDlgItemMessageW",
            "SendDlgItemMessageA",
            "GetDlgItem",
            "GetDlgCtrlID",
            "GetDlgItemInt",
            "SetDlgItemInt",
            "EnumChildWindows",
            "DialogBoxIndirectParamW",
            "CreateDialogIndirectParamW",
            "MapDialogRect",
            "IsDialogMessageW",
            "CreateIconIndirect",
            "SetWindowPlacement",
            "GetScrollInfo",
            "SetScrollInfo",
            "LockWindowUpdate",
            "EnumWindows",
            "EnumThreadWindows",
        ] {
            assert!(
                !wire::x64::runtime::import_supported(dll, name),
                "{dll}!{name}"
            );
        }
    }
}
