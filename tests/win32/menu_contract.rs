//! Preserve the old gateway order and resource/output commits before migration.
use super::*;

fn info(v: &Vm, fields: [u32; 12]) -> u32 {
    let p = DATA + 0x1800;
    for (i, field) in fields.into_iter().enumerate() {
        v.mem.write_u32(p + i as u32 * 4, field).unwrap();
    }
    p
}

#[test]
fn menu_contract_keeps_struct_validation_and_both_output_prechecks() {
    let mut v = Vm::new();
    assert_eq!(
        v.call("user32.dll", "InsertMenuItemW", &[0xdead, 0, 1, 0x30000]),
        0
    );
    assert_eq!(v.state.last_error, 87);
    let p = info(&v, [48, 2, 0, 0, 77, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        v.call("user32.dll", "InsertMenuItemW", &[0xdead, 0, 1, p]),
        0
    );
    assert_eq!(v.state.last_error, 1401);
    let menu = v.call("user32.dll", "CreateMenu", &[]);
    v.wide(DATA + 0x800, "owned");
    let p = info(&v, [48, 0x42, 0, 0, 77, 0, 0, 0, 0, DATA + 0x800, 0, 0]);
    assert_eq!(v.call("user32.dll", "InsertMenuItemW", &[menu, 0, 1, p]), 1);
    let text = DATA + 0x3800;
    v.mem.write_bytes(text, &[0xa5; 16]).unwrap();
    let p = info(&v, [48, 0x42, 0, 0, 0, 0, 0, 0, 0, text, 8, 0]);
    v.mem
        .virtual_protect(p & !0xfff, 0x1000, wire::mem::PAGE_READONLY)
        .unwrap();
    assert_eq!(
        v.call("user32.dll", "GetMenuItemInfoW", &[menu, 0, 1, p]),
        0
    );
    assert_eq!(v.state.last_error, 87);
    assert_eq!(v.mem.read_u32(text).unwrap(), 0xa5a5a5a5);
    assert_eq!(v.mem.read_u32(p + 16).unwrap(), 0);
    assert_eq!(v.mem.read_u32(p + 40).unwrap(), 8);
    v.mem
        .virtual_protect(p & !0xfff, 0x1000, wire::mem::PAGE_READWRITE)
        .unwrap();
    v.mem
        .virtual_protect(text & !0xfff, 0x1000, wire::mem::PAGE_READONLY)
        .unwrap();
    assert_eq!(
        v.call("user32.dll", "GetMenuItemInfoW", &[menu, 0, 1, p]),
        0
    );
    assert_eq!(v.state.last_error, 87);
    assert_eq!(v.mem.read_u32(text).unwrap(), 0xa5a5a5a5);
    assert_eq!(v.mem.read_u32(p + 16).unwrap(), 0);
    assert_eq!(v.mem.read_u32(p + 40).unwrap(), 8);
    v.mem
        .virtual_protect(text & !0xfff, 0x1000, wire::mem::PAGE_READWRITE)
        .unwrap();
    assert_eq!(
        v.call("user32.dll", "GetMenuItemInfoW", &[menu, 0, 1, p]),
        1
    );
    assert_eq!(v.mem.read_wstr(text, 8).unwrap(), "owned");
    assert_eq!(v.mem.read_u32(p + 16).unwrap(), 77);
    assert_eq!(v.mem.read_u32(p + 40).unwrap(), 5);
}

#[test]
fn menu_contract_keeps_shared_submenu_retirement_separate_from_child_ids() {
    let mut v = Vm::new();
    let root = v.call("user32.dll", "CreateMenu", &[]);
    let other = v.call("user32.dll", "CreateMenu", &[]);
    let shared = v.call("user32.dll", "CreatePopupMenu", &[]);
    for menu in [root, other] {
        let p = info(&v, [48, 4, 0, 0, 0, shared, 0, 0, 0, 0, 0, 0]);
        assert_eq!(v.call("user32.dll", "InsertMenuItemW", &[menu, 0, 1, p]), 1);
    }
    let def = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let parent = test_window(&mut v, def);
    assert_eq!(v.call("user32.dll", "SetMenu", &[parent, root]), 1);
    let atom = v.state.user32.windows[&parent].class as u32;
    let child = v.call(
        "user32.dll",
        "CreateWindowExW",
        &[
            0, atom, 0, 0x40000000, 0, 0, 32, 32, parent, shared, 0x400000, 0,
        ],
    );
    assert_ne!(child, 0);
    assert_eq!(v.call("user32.dll", "DestroyMenu", &[shared]), 1);
    for menu in [root, other] {
        assert_eq!(v.call("user32.dll", "IsMenu", &[menu]), 1);
        assert_eq!(v.call("user32.dll", "GetMenuItemCount", &[menu]), 1);
        assert_eq!(v.call("user32.dll", "GetSubMenu", &[menu, 0]), 0);
    }
    assert_eq!(v.call("user32.dll", "GetMenu", &[parent]), root);
    assert_eq!(v.call("user32.dll", "GetDlgCtrlID", &[child]), shared);
    assert_eq!(v.call("user32.dll", "DestroyMenu", &[root]), 1);
    assert_eq!(v.call("user32.dll", "GetMenu", &[parent]), 0);
    assert_eq!(v.call("user32.dll", "GetDlgCtrlID", &[child]), shared);
}

#[test]
fn menu_contract_pins_redraw_gateway_shadowing_and_unimplemented_trace() {
    let mut v = Vm::new();
    let def = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, def);
    assert_eq!(v.call("user32.dll", "ValidateRect", &[hwnd, 0]), 1);
    assert_eq!(v.call("user32.dll", "RedrawWindow", &[hwnd, 0, 0, 1]), 0);
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.call("user32.dll", "GetUpdateRect", &[hwnd, 0, 0]), 0);
    let mut a = [0; 16];
    a[0] = hwnd;
    a[3] = 1;
    let result = wire::user32::dispatch(
        "user32.dll",
        "RedrawWindow",
        &a,
        &mut v.cpu,
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        1_000_000,
    )
    .unwrap()
    .unwrap();
    assert!(matches!(result.0, wire::cpu::GuestCallOutcome::Returned(1)));
    assert_eq!(v.call("user32.dll", "GetUpdateRect", &[hwnd, 0, 0]), 1);
    assert_eq!(v.call("user32.dll", "RedrawWindow", &[hwnd, 0, 0, 8]), 0);
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.call("user32.dll", "GetUpdateRect", &[hwnd, 0, 0]), 1);
    v.log.finish();
    let trace = std::fs::read_to_string(v.dir.join("api.log")).unwrap();
    let calls: Vec<_> = trace
        .lines()
        .filter(|line| line.starts_with('#') && line.contains("user32.dll!RedrawWindow("))
        .collect();
    assert_eq!(calls.len(), 2);
    for line in calls {
        assert!(line.contains("[UNIMPLEMENTED] ; menu/USER32 error 120"));
    }
}
