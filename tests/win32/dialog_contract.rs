//! Independently observed control-wrapper and enumeration failure ordering.
use super::*;

#[test]
fn dialog_contract_keeps_control_lookup_before_output_and_partial_integer_output() {
    let mut v = Vm::new();
    let procedure = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let parent = test_window(&mut v, procedure);
    let atom = v.state.user32.windows[&parent].class as u32;
    let child = v.call(
        "user32.dll",
        "CreateWindowExW",
        &[
            0, atom, 0, 0x40000000, 0, 0, 32, 32, parent, 42, 0x400000, 0,
        ],
    );
    assert_ne!(child, 0);
    assert_eq!(
        v.call("user32.dll", "GetDlgItemInt", &[parent, 43, 0x30000, 1]),
        0
    );
    assert_eq!(v.state.last_error, 1400);
    v.mem.write_u32(DATA + 0x800, 0xa5a5a5a5).unwrap();
    v.state.user32.windows.get_mut(&child).unwrap().tid = 2;
    assert_eq!(
        v.call(
            "user32.dll",
            "GetDlgItemInt",
            &[parent, 42, DATA + 0x800, 1]
        ),
        0
    );
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.mem.read_u32(DATA + 0x800).unwrap(), 0xa5a5a5a5);
    v.state.user32.windows.get_mut(&child).unwrap().tid = 1;
    v.mem.write_bytes(CODE, &[0x0f, 0x0b]).unwrap();
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[child, (-4i32) as u32, CODE],
    );
    assert!(v
        .try_call(
            "user32.dll",
            "GetDlgItemInt",
            &[parent, 42, DATA + 0x800, 1]
        )
        .is_err());
    // The translated flag is cleared before entering WM_GETTEXT, even if
    // that callback faults and no decimal result can be committed.
    assert_eq!(v.mem.read_u32(DATA + 0x800).unwrap(), 0);
}

#[test]
fn dialog_contract_preserves_enumeration_null_checks_and_false_return_carriers() {
    let mut v = Vm::new();
    assert_eq!(v.call("user32.dll", "EnumWindows", &[0, 0]), 0);
    assert_eq!(v.state.last_error, 87);
    assert_eq!(
        v.call("user32.dll", "EnumThreadWindows", &[0xdead, 0, 0]),
        0
    );
    assert_eq!(v.state.last_error, 87);
    assert_eq!(
        v.call("user32.dll", "EnumThreadWindows", &[0xdead, 0x30000, 0]),
        1
    );
    assert_eq!(v.call("user32.dll", "EnumChildWindows", &[0xdead, 0, 0]), 1);
    let procedure = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let parent = test_window(&mut v, procedure);
    let atom = v.state.user32.windows[&parent].class as u32;
    let child = v.call(
        "user32.dll",
        "CreateWindowExW",
        &[
            0, atom, 0, 0x40000000, 0, 0, 32, 32, parent, 42, 0x400000, 0,
        ],
    );
    assert_ne!(child, 0);
    v.mem.write_bytes(CODE, &[0x31, 0xc0, 0xc2, 8, 0]).unwrap();
    assert_eq!(v.call("user32.dll", "EnumWindows", &[CODE, 0]), 0);
    assert_eq!(
        v.call("user32.dll", "EnumChildWindows", &[parent, CODE, 0]),
        1
    );
}

#[test]
fn dialog_contract_keeps_explicit_unavailable_calls_before_guest_access() {
    let mut v = Vm::new();
    for name in [
        "DialogBoxIndirectParamW",
        "CreateDialogIndirectParamW",
        "MapDialogRect",
        "IsDialogMessageW",
        "CreateIconIndirect",
        "SetWindowPlacement",
        "GetScrollInfo",
        "SetScrollInfo",
        "LockWindowUpdate",
    ] {
        v.state.last_error = 0;
        assert_eq!(v.call("user32.dll", name, &[0x30000; 5]), 0, "{name}");
        assert_eq!(v.state.last_error, 120, "{name}");
    }
}
