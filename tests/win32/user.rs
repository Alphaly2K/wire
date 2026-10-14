//! Existing behavioral tests grouped by user; expectations remain independent of API catalogs.

use super::*;

fn menu_info(v: &Vm, fields: [u32; 12]) -> u32 {
    for (i, n) in fields.into_iter().enumerate() {
        v.mem.write_u32(DATA + 0x800 + i as u32 * 4, n).unwrap();
    }
    DATA + 0x800
}

#[test]
fn cursor_show_count_is_signed_and_set_cursor_returns_previous_handle() {
    let mut v = Vm::new();
    assert_eq!(v.call("user32.dll", "ShowCursor", &[1]), 1);
    assert_eq!(v.call("user32.dll", "ShowCursor", &[0]), 0);
    assert_eq!(v.call("user32.dll", "ShowCursor", &[0]), u32::MAX);
    assert_eq!(v.call("user32.dll", "ShowCursor", &[1]), 0);
    let arrow = v.call("user32.dll", "LoadCursorA", &[0, 32512]);
    assert_ne!(arrow, 0);
    assert_eq!(v.call("user32.dll", "SetCursor", &[arrow]), 0);
    assert_eq!(v.call("user32.dll", "GetCursor", &[]), arrow);
    assert_eq!(v.call("user32.dll", "SetCursor", &[0]), arrow);
    assert_eq!(v.call("user32.dll", "GetCursor", &[]), 0);
    assert_eq!(v.call("user32.dll", "SetCursor", &[0xdeadbeef]), 0);
    assert_eq!(v.state.last_error, 6);
}

#[test]
fn monitor_callbacks_and_ansi_info_return_real_bounds_and_honor_clipping() {
    let mut v = Vm::new();
    let mut code = vec![0xff, 0x05];
    code.extend((DATA + 0x300).to_le_bytes());
    code.extend([0x8b, 0x44, 0x24, 12, 0x8b, 0x40, 8, 0xa3]);
    code.extend((DATA + 0x304).to_le_bytes());
    code.extend([0xb8, 0, 0, 0, 0, 0xc2, 16, 0]);
    v.mem.write_bytes(CODE, &code).unwrap();
    assert_eq!(
        v.call("user32.dll", "EnumDisplayMonitors", &[0, 0, CODE, 0]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x304).unwrap(), 1024);
    for (i, n) in [2000u32, 0, 2100, 100].into_iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, n).unwrap();
    }
    assert_eq!(
        v.call("user32.dll", "EnumDisplayMonitors", &[0, DATA, CODE, 0]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), 1);
    for (name, size) in [
        ("GetMonitorInfoA", 40),
        ("GetMonitorInfoA", 72),
        ("GetMonitorInfoW", 104),
    ] {
        v.mem.write_u32(DATA, size).unwrap();
        assert_eq!(v.call("user32.dll", name, &[0x40000001, DATA]), 1);
        assert_eq!(v.mem.read_u32(DATA + 12).unwrap(), 1024);
        assert_eq!(v.mem.read_u32(DATA + 16).unwrap(), 768);
        if size == 72 {
            assert_eq!(v.mem.read_cstr(DATA + 40, 32).unwrap(), "\\\\.\\DISPLAY1");
        }
        if size == 104 {
            assert_eq!(v.mem.read_wstr(DATA + 40, 32).unwrap(), "\\\\.\\DISPLAY1");
        }
    }
}

#[test]
fn frame_adjustment_client_geometry_and_paint_clipping_share_the_same_pixels() {
    let mut v = Vm::new();
    let def = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, def);
    v.call("user32.dll", "SetRect", &[DATA + 0x800, 0, 0, 200, 100]);
    assert_eq!(
        v.call(
            "user32.dll",
            "AdjustWindowRectEx",
            &[DATA + 0x800, 0xc40000, 0, 0]
        ),
        1
    );
    for (i, n) in [-4i32, -27, 204, 104].into_iter().enumerate() {
        assert_eq!(
            v.mem.read_u32(DATA + 0x800 + i as u32 * 4).unwrap() as i32,
            n
        );
    }
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-16i32) as u32, 0xc40000],
    );
    v.call(
        "user32.dll",
        "SetWindowPos",
        &[hwnd, 0, 30, 40, 208, 131, 0x14],
    );
    v.call("user32.dll", "GetClientRect", &[hwnd, DATA + 0x820]);
    assert_eq!(v.mem.read_u32(DATA + 0x828).unwrap(), 200);
    assert_eq!(v.mem.read_u32(DATA + 0x82c).unwrap(), 100);
    let class = v.state.user32.windows[&hwnd].class;
    v.state.user32.classes.get_mut(&class).unwrap().fields[7] = 6;
    v.call("user32.dll", "ShowWindow", &[hwnd, 1]);
    assert_eq!(
        v.call("user32.dll", "GetUpdateRect", &[hwnd, DATA + 0x840, 0]),
        1
    );
    let dc = v.call("user32.dll", "BeginPaint", &[hwnd, DATA + 0x880]);
    assert_ne!(dc, 0);
    assert_eq!(v.mem.read_u32(DATA + 0x884).unwrap(), 0); // background was truly erased
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), 0xffffff);
    assert_eq!(v.call("user32.dll", "GetUpdateRect", &[hwnd, 0, 0]), 0);
    assert_eq!(v.call("user32.dll", "EndPaint", &[hwnd, DATA + 0x880]), 1);
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), u32::MAX);
    v.call("user32.dll", "SetRect", &[DATA + 0x800, 10, 20, 14, 22]);
    v.call("user32.dll", "InvalidateRect", &[hwnd, DATA + 0x800, 0]);
    let dc = v.call("user32.dll", "BeginPaint", &[hwnd, DATA + 0x880]);
    assert_eq!(v.call("gdi32.dll", "SetPixel", &[dc, 10, 20, 0xff]), 0xff);
    assert_eq!(v.call("gdi32.dll", "SetPixel", &[dc, 0, 0, 0xff]), u32::MAX);
    v.call("user32.dll", "EndPaint", &[hwnd, DATA + 0x880]);
    let (w, h, pixels) = v.state.graphics.window_snapshot(&v.mem, hwnd).unwrap();
    assert_eq!((w, h), (200, 100));
    assert_eq!(pixels[20 * 200 + 10], 0xff);
    assert_eq!(pixels[0], 0xffffff);
}

#[test]
fn default_window_position_messages_use_actual_client_changes_and_destroy_clears_input_owners() {
    let mut v = Vm::new();
    // Record lParam indexed by message, and accept WM_NCCREATE.
    let mut proc = vec![0x8b, 0x44, 0x24, 8, 0x8b, 0x54, 0x24, 16, 0x89, 0x14, 0x85];
    proc.extend((DATA + 0x1000).to_le_bytes());
    proc.extend([
        0x3d, 0x81, 0, 0, 0, 0x0f, 0x94, 0xc0, 0x0f, 0xb6, 0xc0, 0xc2, 16, 0,
    ]);
    v.mem.write_bytes(CODE, &proc).unwrap();
    let hwnd = test_window(&mut v, CODE);
    assert_ne!(hwnd, 0);
    let p = DATA + 0x800;
    for (i, n) in [hwnd, 0, 3, 4, 640, 480, 0].into_iter().enumerate() {
        v.mem.write_u32(p + i as u32 * 4, n).unwrap();
    }
    v.call("user32.dll", "DefWindowProcW", &[hwnd, 0x47, 0, p]);
    assert_eq!(
        v.mem.read_u32(DATA + 0x1000 + 3 * 4).unwrap(),
        3 | (4 << 16)
    );
    assert_eq!(
        v.mem.read_u32(DATA + 0x1000 + 5 * 4).unwrap(),
        640 | (480 << 16)
    );
    v.mem.write_u32(DATA + 0x1000 + 5 * 4, 0xdeadbeef).unwrap();
    v.mem.write_u32(p + 24, 0x1800).unwrap();
    v.call("user32.dll", "DefWindowProcW", &[hwnd, 0x47, 0, p]);
    assert_eq!(v.mem.read_u32(DATA + 0x1000 + 5 * 4).unwrap(), 0xdeadbeef);
    v.call("user32.dll", "SetFocus", &[hwnd]);
    v.call("user32.dll", "SetActiveWindow", &[hwnd]);
    v.call("user32.dll", "SetCapture", &[hwnd]);
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 1);
    for name in [
        "GetFocus",
        "GetActiveWindow",
        "GetForegroundWindow",
        "GetCapture",
    ] {
        assert_eq!(v.call("user32.dll", name, &[]), 0);
    }
}

#[test]
fn menus_copy_guest_strings_find_nested_commands_and_preserve_masked_fields() {
    let mut v = Vm::new();
    let root = v.call("user32.dll", "CreateMenu", &[]);
    let child = v.call("user32.dll", "CreatePopupMenu", &[]);
    assert_ne!(root, child);
    assert_ne!(child, 0);
    v.wide(DATA + 0x900, "日本語");
    let p = menu_info(
        &v,
        [44, 0x33, 0, 8, 77, 0, 0, 0, 0x1234, DATA + 0x900, 1, 0],
    );
    assert_eq!(
        v.call("user32.dll", "InsertMenuItemW", &[child, u32::MAX, 1, p]),
        1
    );
    v.wide(DATA + 0x900, "changed"); // menu owns a copy; cch is ignored on input
    let p = menu_info(&v, [48, 6, 0, 0, 77, child, 0, 0, 0, 0, 0, 0]);
    assert_eq!(v.call("user32.dll", "InsertMenuItemW", &[root, 0, 1, p]), 1);
    assert_eq!(v.call("user32.dll", "GetSubMenu", &[root, 0]), child);
    assert_eq!(v.call("user32.dll", "GetMenuItemID", &[root, 0]), u32::MAX);
    // Descendant command 77 takes priority over popup command 77.
    let p = menu_info(&v, [48, 0x63, 0, 0, 0, 0, 0, 0, 0, DATA + 0xa00, 3, 0]);
    assert_eq!(
        v.call("user32.dll", "GetMenuItemInfoW", &[root, 77, 0, p]),
        1
    );
    assert_eq!(v.mem.read_wstr(DATA + 0xa00, 10).unwrap(), "日本");
    assert_eq!(v.mem.read_u32(p + 40).unwrap(), 2);
    assert_eq!(v.mem.read_u32(p + 32).unwrap(), 0x1234);
    assert_eq!(v.mem.read_u32(p + 12).unwrap(), 8);
    let p = menu_info(&v, [48, 1, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        v.call("user32.dll", "SetMenuItemInfoW", &[root, 77, 0, p]),
        1
    );
    let p = menu_info(&v, [48, 0x62, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        v.call("user32.dll", "GetMenuItemInfoA", &[root, 77, 0, p]),
        1
    );
    assert_eq!(v.mem.read_u32(p + 40).unwrap(), 6); // CP932 bytes
    assert_eq!(v.mem.read_u32(p + 16).unwrap(), 77);
    assert_eq!(v.mem.read_u32(p + 32).unwrap(), 0x1234);
    assert_eq!(v.call("user32.dll", "EnableMenuItem", &[root, 77, 0]), 3);
    assert_eq!(v.call("user32.dll", "CheckMenuItem", &[root, 77, 8]), 0);
    assert_eq!(v.call("user32.dll", "GetMenuState", &[root, 77, 0]), 8);
    assert_eq!(v.call("user32.dll", "RemoveMenu", &[root, 0, 0x400]), 1);
    assert_eq!(v.call("user32.dll", "GetMenuItemCount", &[root]), 0);
    assert_eq!(v.call("user32.dll", "IsMenu", &[child]), 1); // Remove does not destroy
    let p = menu_info(&v, [48, 4, 0, 0, 0, child, 0, 0, 0, 0, 0, 0]);
    assert_eq!(v.call("user32.dll", "InsertMenuItemW", &[root, 0, 1, p]), 1);
    assert_eq!(v.call("user32.dll", "DeleteMenu", &[root, 0, 0x400]), 1);
    assert_eq!(v.call("user32.dll", "IsMenu", &[child]), 0);
    assert_eq!(v.call("user32.dll", "GetMenuItemCount", &[child]), u32::MAX);
    assert_eq!(v.k("GetLastError", &[]), 1401);
}

#[test]
fn menus_reject_invalid_structs_pointers_cycles_and_deep_graphs_without_partial_mutation() {
    let mut v = Vm::new();
    let root = v.call("user32.dll", "CreateMenu", &[]);
    for fields in [
        [40, 2, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [48, 0x50, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        [48, 4, 0, 0, 1, root, 0, 0, 0, 0, 0, 0],
        [48, 0x40, 0, 0, 1, 0, 0, 0, 0, 0x30000000, 0, 0],
    ] {
        let p = menu_info(&v, fields);
        assert_eq!(v.call("user32.dll", "InsertMenuItemW", &[root, 0, 1, p]), 0);
        assert_eq!(v.k("GetLastError", &[]), 87);
        assert_eq!(v.call("user32.dll", "GetMenuItemCount", &[root]), 0);
    }
    let p = menu_info(&v, [48, 2, 0x800, 0, 42, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(v.call("user32.dll", "InsertMenuItemW", &[root, 0, 1, p]), 1);
    let p = menu_info(&v, [48, 4, 0, 0, 0, root, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        v.call("user32.dll", "SetMenuItemInfoW", &[root, 0, 1, p]),
        0
    );
    assert_eq!(v.call("user32.dll", "GetMenuItemID", &[root, 0]), 42);
    let p = menu_info(&v, [48, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    v.mem
        .virtual_protect(DATA, 0x10000, wire::mem::PAGE_READONLY)
        .unwrap();
    assert_eq!(
        v.call("user32.dll", "GetMenuItemInfoW", &[root, 0, 1, p]),
        0
    );
    assert_eq!(v.mem.read_u32(p + 16).unwrap(), 0);
    v.mem
        .virtual_protect(DATA, 0x10000, wire::mem::PAGE_READWRITE)
        .unwrap();
    let mut parent = root;
    for _ in 1..30 {
        let child = v.call("user32.dll", "CreatePopupMenu", &[]);
        let p = menu_info(&v, [48, 4, 0, 0, 0, child, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            v.call("user32.dll", "InsertMenuItemW", &[parent, u32::MAX, 1, p]),
            1
        );
        parent = child;
    }
    let child = v.call("user32.dll", "CreatePopupMenu", &[]);
    let p = menu_info(&v, [48, 4, 0, 0, 0, child, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        v.call("user32.dll", "InsertMenuItemW", &[parent, 0, 1, p]),
        0
    );
    assert_eq!(v.call("user32.dll", "GetMenuItemCount", &[parent]), 0);
    assert_eq!(v.call("user32.dll", "DestroyMenu", &[root]), 1);
    assert_eq!(v.call("user32.dll", "IsMenu", &[parent]), 0);
    assert_eq!(v.call("user32.dll", "IsMenu", &[child]), 1);
}

#[test]
fn window_menu_attachment_lifetime_and_coordinate_roundtrip_are_real() {
    let mut v = Vm::new();
    let def = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, def);
    let menu = v.call("user32.dll", "CreateMenu", &[]);
    assert_eq!(v.call("user32.dll", "SetMenu", &[hwnd, menu]), 1);
    assert_eq!(v.call("user32.dll", "GetMenu", &[hwnd]), menu);
    assert_eq!(v.call("user32.dll", "SetMenu", &[hwnd, 123]), 0);
    assert_eq!(v.call("user32.dll", "GetMenu", &[hwnd]), menu);
    v.mem.write_u32(DATA + 0xb00, (-10i32) as u32).unwrap();
    v.mem.write_u32(DATA + 0xb04, 20).unwrap();
    assert_eq!(
        v.call("user32.dll", "ClientToScreen", &[hwnd, DATA + 0xb00]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0xb00).unwrap() as i32, -7);
    assert_eq!(v.mem.read_u32(DATA + 0xb04).unwrap(), 43); // client begins below the menu
    assert_eq!(
        v.call("user32.dll", "ScreenToClient", &[hwnd, DATA + 0xb00]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0xb00).unwrap() as i32, -10);
    assert_eq!(v.mem.read_u32(DATA + 0xb04).unwrap(), 20);
    assert_eq!(
        v.call("user32.dll", "TrackPopupMenuEx", &[menu, 0, 0, 0, hwnd, 0]),
        0
    );
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 28); // 6 DWORD stdcall
    assert_eq!(v.k("GetLastError", &[]), 120);
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 1);
    assert_eq!(v.call("user32.dll", "IsMenu", &[menu]), 0);
}

#[test]
fn window_text_dialog_numbers_and_geometry_use_real_control_state() {
    let mut v = Vm::new();
    let def = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let parent = test_window(&mut v, def);
    let atom = v.state.user32.windows[&parent].class as u32;
    let ctrl = v.call(
        "user32.dll",
        "CreateWindowExW",
        &[
            0, atom, 0, 0x40000000, 10, 20, 100, 30, parent, 42, 0x400000, 0,
        ],
    );
    assert_ne!(ctrl, 0);
    assert_eq!(v.call("user32.dll", "GetDlgItem", &[parent, 42]), ctrl);
    assert_eq!(v.call("user32.dll", "GetDlgCtrlID", &[ctrl]), 42);
    assert_eq!(
        v.call(
            "user32.dll",
            "SetDlgItemInt",
            &[parent, 42, (-123i32) as u32, 1]
        ),
        1
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "GetDlgItemInt",
            &[parent, 42, DATA + 0xc00, 1]
        ),
        (-123i32) as u32
    );
    assert_eq!(v.mem.read_u32(DATA + 0xc00).unwrap(), 1);
    assert_eq!(v.call("user32.dll", "GetWindowTextLengthW", &[ctrl]), 4);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetDlgItemTextW",
            &[parent, 42, DATA + 0xc10, 3]
        ),
        2
    );
    assert_eq!(v.mem.read_wstr(DATA + 0xc10, 10).unwrap(), "-1");
    v.wide(DATA + 0xc30, "2147483648");
    assert_eq!(
        v.call("user32.dll", "SetWindowTextW", &[ctrl, DATA + 0xc30]),
        1
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "GetDlgItemInt",
            &[parent, 42, DATA + 0xc00, 1]
        ),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0xc00).unwrap(), 0);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetDlgItemInt",
            &[parent, 42, DATA + 0xc00, 0]
        ),
        0x80000000
    );
    assert_eq!(
        v.call("user32.dll", "GetClassNameW", &[ctrl, DATA + 0xc30, 32]),
        14
    );
    assert_eq!(v.mem.read_wstr(DATA + 0xc30, 32).unwrap(), "CallbackWindow");
    assert_eq!(
        v.call("user32.dll", "MoveWindow", &[ctrl, 12, 22, 120, 40, 1]),
        1
    );
    v.mem.write_bytes(DATA + 0xc60, &[0; 8]).unwrap();
    assert_eq!(
        v.call("user32.dll", "MapWindowPoints", &[ctrl, 0, DATA + 0xc60, 1]),
        15 | (26 << 16)
    );
    assert_eq!(v.mem.read_u32(DATA + 0xc60).unwrap(), 15);
    assert_eq!(v.mem.read_u32(DATA + 0xc64).unwrap(), 26);
    assert_eq!(v.call("user32.dll", "IsWindowEnabled", &[ctrl]), 1);
    assert_eq!(v.call("user32.dll", "EnableWindow", &[ctrl, 0]), 0);
    assert_eq!(v.call("user32.dll", "IsWindowEnabled", &[ctrl]), 0);
    assert_eq!(v.call("user32.dll", "SetFocus", &[ctrl]), 0);
    assert_eq!(v.call("user32.dll", "GetFocus", &[]), ctrl);
    assert_eq!(v.call("user32.dll", "SetParent", &[parent, ctrl]), 0);
    assert_eq!(v.k("GetLastError", &[]), 87);
}

#[test]
fn window_classes_keep_metadata_support_atoms_and_share_ansi_unicode_names() {
    let mut v = Vm::new();
    v.wide(DATA + 128, "WireWindow");
    v.mem.write_bytes(DATA + 160, b"wirewindow\0").unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExW",
            &[0x400000, DATA + 128, DATA + 256]
        ),
        0
    );
    assert_eq!(v.state.last_error, 1411);
    let words = [48, 3, CODE, 4, 8, 0x400000, 2, 3, 4, 0, DATA + 128, 5];
    for (i, value) in words.into_iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, value).unwrap();
    }
    let atom = v.call("user32.dll", "RegisterClassExW", &[DATA]);
    assert!((0xc000..=0xffff).contains(&atom));
    assert_eq!(v.call("user32.dll", "RegisterClassExW", &[DATA]), 0);
    assert_eq!(v.state.last_error, 1410);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExA",
            &[0x400000, DATA + 160, DATA + 256]
        ),
        1
    );
    for (i, value) in words.into_iter().enumerate().filter(|(i, _)| *i != 10) {
        assert_eq!(v.mem.read_u32(DATA + 256 + i as u32 * 4).unwrap(), value);
    }
    let name = v.mem.read_u32(DATA + 296).unwrap();
    assert_eq!(v.mem.read_cstr(name, 256).unwrap(), "WireWindow");
    assert_eq!(
        v.call("user32.dll", "GetClassInfoW", &[0x400000, atom, DATA + 320]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 324).unwrap(), CODE);
    let name = v.mem.read_u32(DATA + 356).unwrap();
    assert_eq!(v.mem.read_wstr(name, 256).unwrap(), "WireWindow");
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExA",
            &[0x500000, atom, DATA + 256]
        ),
        0
    );
    assert_eq!(
        v.call("user32.dll", "UnregisterClassA", &[atom, 0x400000]),
        1
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExW",
            &[0x400000, DATA + 128, DATA + 256]
        ),
        0
    );
    // WNDCLASS (40 bytes) has no cbSize/hIconSm and remains interoperable.
    for (i, value) in words[1..11].iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, *value).unwrap();
    }
    let atom = v.call("user32.dll", "RegisterClassW", &[DATA]);
    assert_ne!(atom, 0);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoExW",
            &[0x400000, atom, DATA + 256]
        ),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 300).unwrap(), 0);
}

#[test]
fn keyboard_host_state_and_retrieved_state_follow_different_boundaries() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
        .unwrap();
    let hwnd = test_window(&mut v, CODE);
    wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x100, 0x41, 1);
    assert_eq!(
        v.call("user32.dll", "GetAsyncKeyState", &[0x41]),
        0xffff8001
    );
    assert_eq!(
        v.call("user32.dll", "GetAsyncKeyState", &[0x41]),
        0xffff8000
    );
    assert_eq!(v.call("user32.dll", "GetKeyState", &[0x41]), 0);
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0x100, 0x100, 0]
        ),
        1
    );
    assert_eq!(v.call("user32.dll", "GetKeyState", &[0x41]), 0);
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0x100, 0x100, 1]
        ),
        1
    );
    assert_eq!(v.call("user32.dll", "GetKeyState", &[0x41]), 0xffff8000);
    assert_eq!(v.call("user32.dll", "GetKeyboardState", &[DATA + 0xa00]), 1);
    assert_eq!(v.mem.read_u8(DATA + 0xa41).unwrap(), 0x80);
    wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x101, 0x41, 0xc0000001);
    assert_eq!(v.call("user32.dll", "GetAsyncKeyState", &[0x41]), 0);
    assert_eq!(v.call("user32.dll", "GetKeyState", &[0x41]), 0xffff8000);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetMessageW",
            &[DATA + 0x900, hwnd, 0x101, 0x101]
        ),
        1
    );
    assert_eq!(v.call("user32.dll", "GetKeyState", &[0x41]), 0);
    assert_eq!(v.call("user32.dll", "GetAsyncKeyState", &[256]), 0);
    assert!(v
        .try_call("user32.dll", "GetKeyboardState", &[0xfffffff0])
        .is_err());
    // CapsLock toggles once per physical press, not again on auto-repeat.
    for lp in [1, 1 | (1 << 30)] {
        wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x100, 0x14, lp);
        v.call(
            "user32.dll",
            "GetMessageW",
            &[DATA + 0x900, hwnd, 0x100, 0x100],
        );
        assert_eq!(v.call("user32.dll", "GetKeyState", &[0x14]), 0xffff8001);
    }
    wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x101, 0x14, 0xc0000001);
    v.call(
        "user32.dll",
        "GetMessageW",
        &[DATA + 0x900, hwnd, 0x101, 0x101],
    );
    assert_eq!(v.call("user32.dll", "GetKeyState", &[0x14]), 1);
}

#[test]
fn keyboard_translation_defers_host_text_and_preserves_unicode_and_ansi() {
    for wide in [true, false] {
        let mut v = Vm::new();
        v.mem
            .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
            .unwrap();
        let hwnd = if wide {
            test_window(&mut v, CODE)
        } else {
            v.mem.write_bytes(DATA + 128, b"AnsiKeyboard\0").unwrap();
            for (i, n) in [48, 0, CODE, 0, 0, 0x400000, 0, 0, 0, 0, DATA + 128, 0]
                .into_iter()
                .enumerate()
            {
                v.mem.write_u32(DATA + i as u32 * 4, n).unwrap();
            }
            let atom = v.call("user32.dll", "RegisterClassExA", &[DATA]);
            v.call(
                "user32.dll",
                "CreateWindowExA",
                &[0, atom, 0, 0, 0, 0, 640, 480, 0, 0, 0x400000, 0],
            )
        };
        assert_ne!(hwnd, 0);
        // The host layout resolves text; the guest virtual key does not imply a US character.
        for (msg, key, lp, text) in [(0x100, 0x51, 1, "あ"), (0x104, 0x42, 0x20000001, "B")] {
            wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, msg, key, lp);
            for c in text.encode_utf16() {
                wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x102, c as u32, lp);
            }
            let character = if msg == 0x104 { 0x106 } else { 0x102 };
            assert_eq!(
                v.call(
                    "user32.dll",
                    "PeekMessageW",
                    &[DATA + 0x900, hwnd, character, character, 1]
                ),
                0
            );
            v.call("user32.dll", "GetMessageW", &[DATA + 0x900, hwnd, msg, msg]);
            assert_eq!(v.call("user32.dll", "TranslateMessage", &[DATA + 0x900]), 1);
            let expected: Vec<u32> = if wide {
                text.encode_utf16().map(u32::from).collect()
            } else {
                wire::codepage::encode(text, 932)
                    .unwrap()
                    .0
                    .into_iter()
                    .map(u32::from)
                    .collect()
            };
            for c in expected {
                assert_eq!(
                    v.call(
                        "user32.dll",
                        "PeekMessageW",
                        &[DATA + 0x900, hwnd, character, character, 1]
                    ),
                    1
                );
                assert_eq!(v.mem.read_u32(DATA + 0x908).unwrap(), c);
                assert_eq!(v.mem.read_u32(DATA + 0x90c).unwrap(), lp);
            }
            assert_eq!(
                v.call(
                    "user32.dll",
                    "PeekMessageW",
                    &[DATA + 0x900, hwnd, character, character, 1]
                ),
                0
            );
        }
    }
}

#[test]
fn keyboard_posted_messages_do_not_fake_physical_key_state() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
        .unwrap();
    let hwnd = test_window(&mut v, CODE);
    v.call("user32.dll", "PostMessageW", &[hwnd, 0x100, 13, 1]);
    v.call(
        "user32.dll",
        "GetMessageW",
        &[DATA + 0x900, hwnd, 0x100, 0x100],
    );
    assert_eq!(v.call("user32.dll", "GetKeyState", &[13]), 0);
    assert_eq!(v.call("user32.dll", "GetAsyncKeyState", &[13]), 0);
    assert_eq!(v.call("user32.dll", "TranslateMessage", &[DATA + 0x900]), 1);
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0x102, 0x102, 1]
        ),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0x908).unwrap(), 13);
    assert_eq!(v.call("user32.dll", "TranslateMessage", &[DATA + 0x900]), 0);
}

#[test]
fn window_properties_default_classes_and_registered_names_have_lifetimes() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA + 0x400, b"Static\0").unwrap();
    let hwnd = v.call(
        "user32.dll",
        "CreateWindowExA",
        &[0, DATA + 0x400, 0, 0, 0, 0, 30, 20, 0, 0, 0x400000, 0],
    );
    assert_ne!(hwnd, 0);
    v.mem.write_bytes(DATA + 0x500, b"EngineContext\0").unwrap();
    assert_eq!(
        v.call("user32.dll", "SetPropA", &[hwnd, DATA + 0x500, 0x12345678]),
        1
    );
    v.wide(DATA + 0x600, "EngineContext");
    assert_eq!(
        v.call("user32.dll", "GetPropW", &[hwnd, DATA + 0x600]),
        0x12345678
    );
    assert_eq!(
        v.call("user32.dll", "RemovePropW", &[hwnd, DATA + 0x600]),
        0x12345678
    );
    assert_eq!(v.call("user32.dll", "GetPropA", &[hwnd, DATA + 0x500]), 0);
    assert_eq!(v.call("user32.dll", "SetPropW", &[hwnd, 123, 456]), 1);
    assert_eq!(v.call("user32.dll", "GetPropA", &[hwnd, 123]), 456);
    let id = v.call("user32.dll", "RegisterWindowMessageA", &[DATA + 0x500]);
    assert!(id >= 0xc000);
    assert_eq!(
        v.call("user32.dll", "RegisterWindowMessageW", &[DATA + 0x600]),
        id
    );
    let desktop = v.call("user32.dll", "GetDesktopWindow", &[]);
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[desktop]), 0);
    assert_eq!(v.call("user32.dll", "IsWindow", &[desktop]), 1);
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 1);
    assert_eq!(v.call("user32.dll", "GetPropA", &[hwnd, 123]), 0);
}

#[test]
fn display_enumeration_has_consistent_modes_and_checked_output_layouts() {
    let mut v = Vm::new();
    for wide in [false, true] {
        let suffix = if wide { "W" } else { "A" };
        let size = if wide { 840 } else { 424 };
        v.mem.write_u32(DATA, size).unwrap();
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("EnumDisplayDevices{suffix}"),
                &[0, 0, DATA, 0]
            ),
            1
        );
        assert_eq!(
            wire::file_api::read_name(&v.mem, DATA + 4, wide).unwrap(),
            "\\\\.\\DISPLAY1"
        );
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("EnumDisplayDevices{suffix}"),
                &[0, 1, DATA, 0]
            ),
            0
        );
        let dm = DATA + 0x1000;
        v.mem
            .write_u16(
                dm + if wide { 68 } else { 36 },
                if wide { 220 } else { 156 },
            )
            .unwrap();
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("EnumDisplaySettings{suffix}"),
                &[0, u32::MAX, dm]
            ),
            1
        );
        let bits = dm + if wide { 168 } else { 104 };
        assert_eq!(v.mem.read_u32(bits).unwrap(), 32);
        assert_eq!(v.mem.read_u32(bits + 4).unwrap(), 1024);
        assert_eq!(v.mem.read_u32(bits + 8).unwrap(), 768);
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("EnumDisplaySettings{suffix}"),
                &[0, 1, dm]
            ),
            0
        );
        v.mem.write_u16(dm + if wide { 68 } else { 36 }, 8).unwrap();
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("EnumDisplaySettings{suffix}"),
                &[0, 0, dm]
            ),
            0
        );
        assert_eq!(v.state.last_error, 87);
    }
}

#[test]
fn windows_invoke_creation_forwarding_and_destruction_callbacks() {
    let mut v = Vm::new();
    // Record message/count; return TRUE only for WM_NCCREATE, stdcall 16.
    let mut proc = vec![0x8b, 0x44, 0x24, 8, 0xa3];
    proc.extend((DATA + 256).to_le_bytes());
    proc.extend([0xff, 0x05]);
    proc.extend((DATA + 260).to_le_bytes());
    proc.extend([0x8b, 0x54, 0x24, 16, 0x89, 0x15]);
    proc.extend((DATA + 264).to_le_bytes());
    proc.extend([
        0x3d, 0x81, 0, 0, 0, 0x0f, 0x94, 0xc0, 0x0f, 0xb6, 0xc0, 0xc2, 16, 0,
    ]);
    v.mem.write_bytes(CODE, &proc).unwrap();
    let hwnd = test_window(&mut v, CODE);
    assert_ne!(hwnd, 0);
    assert_eq!(v.mem.read_u32(DATA + 256).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 260).unwrap(), 2);
    assert_eq!(v.call("user32.dll", "IsWindow", &[hwnd]), 1);
    assert_eq!(v.call("user32.dll", "IsWindowUnicode", &[hwnd]), 1);
    assert_eq!(
        v.call(
            "user32.dll",
            "SetWindowLongA",
            &[hwnd, (-21i32) as u32, 0xfeed]
        ),
        0
    );
    assert_eq!(
        v.call("user32.dll", "GetWindowLongW", &[hwnd, (-21i32) as u32]),
        0xfeed
    );
    assert_eq!(
        v.call("user32.dll", "SetWindowLongW", &[hwnd, 4, 0xabcd]),
        0
    );
    assert_eq!(v.call("user32.dll", "GetWindowLongW", &[hwnd, 4]), 0xabcd);
    assert_eq!(v.call("user32.dll", "GetWindowLongW", &[hwnd, 8]), 0);
    assert_eq!(v.state.last_error, 1413);
    assert_eq!(
        v.call("user32.dll", "GetClientRect", &[hwnd, DATA + 320]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 328).unwrap(), 640);
    assert_eq!(v.mem.read_u32(DATA + 332).unwrap(), 480);
    v.mem
        .write_bytes(CODE + 128, &[0x8b, 0x44, 0x24, 12, 0x40, 0xc2, 16, 0])
        .unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "SetWindowLongW",
            &[hwnd, (-4i32) as u32, CODE + 128]
        ),
        CODE
    );
    assert_eq!(
        v.call("user32.dll", "SendMessageW", &[hwnd, 0x401, 42, 0]),
        43
    );
    assert_eq!(
        v.call("user32.dll", "CallWindowProcW", &[CODE, hwnd, 0x402, 0, 0]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 256).unwrap(), 0x402);
    assert_eq!(
        v.call("user32.dll", "UnregisterClassW", &[DATA + 128, 0x400000]),
        0
    );
    assert_eq!(v.state.last_error, 1412);
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-4i32) as u32, CODE],
    );
    let before = v.mem.read_u32(DATA + 260).unwrap();
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 1);
    assert_eq!(v.mem.read_u32(DATA + 256).unwrap(), 0x82);
    assert_eq!(v.mem.read_u32(DATA + 260).unwrap(), before + 2);
    assert_eq!(v.call("user32.dll", "IsWindow", &[hwnd]), 0);
    // An ANSI caller creating a Unicode class supplies a converted CREATESTRUCT.
    v.mem.write_bytes(DATA + 160, b"mixed title\0").unwrap();
    v.mem.write_bytes(DATA + 192, b"MissingClass\0").unwrap();
    let another = v.call(
        "user32.dll",
        "CreateWindowExA",
        &[
            0,
            DATA + 160 + 32,
            DATA + 160,
            0,
            0,
            0,
            10,
            10,
            0,
            0,
            0x400000,
            0x2222,
        ],
    );
    // Unknown class is rejected rather than creating a generic placeholder.
    assert_eq!(another, 0);
    v.mem.write_bytes(DATA + 192, b"CallbackWindow\0").unwrap();
    let another = v.call(
        "user32.dll",
        "CreateWindowExA",
        &[
            0,
            DATA + 192,
            DATA + 160,
            0,
            0,
            0,
            10,
            10,
            0,
            0,
            0x400000,
            0x2222,
        ],
    );
    assert_ne!(another, 0);
    assert_eq!(v.call("user32.dll", "IsWindowUnicode", &[another]), 1);
    let cs = v.mem.read_u32(DATA + 264).unwrap();
    assert_eq!(v.mem.read_u32(cs).unwrap(), 0x2222);
    let title = v.mem.read_u32(cs + 36).unwrap();
    assert_eq!(v.mem.read_wstr(title, 64).unwrap(), "mixed title");
    v.call("user32.dll", "DestroyWindow", &[another]);
    assert_eq!(
        v.call("user32.dll", "UnregisterClassW", &[DATA + 128, 0x400000]),
        1
    );
    v.mem.write_bytes(CODE, &[0x31, 0xc0, 0xc2, 16, 0]).unwrap();
    assert_eq!(test_window(&mut v, CODE), 0);
    assert!(v.state.user32.windows.is_empty());
}

#[test]
fn message_filters_quit_and_timers_preserve_queue_and_callback_semantics() {
    let mut v = Vm::new();
    let def = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, def);
    let msg = DATA + 512;
    v.mem.write_u32(msg, 0xeeee).unwrap();
    assert_eq!(v.call("user32.dll", "PeekMessageW", &[msg, 0, 0, 0, 1]), 0);
    assert_eq!(v.mem.read_u32(msg).unwrap(), 0xeeee);
    v.call("user32.dll", "PostMessageW", &[hwnd, 0x400, 11, 22]);
    v.call("user32.dll", "PostMessageA", &[hwnd, 0x401, 33, 44]);
    assert_eq!(
        v.call("user32.dll", "PeekMessageW", &[msg, hwnd, 0x401, 0x401, 0]),
        1
    );
    assert_eq!(v.mem.read_u32(msg + 8).unwrap(), 33);
    assert_eq!(v.call("user32.dll", "GetMessageA", &[msg, 0, 0, 0]), 1);
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x400);
    assert_eq!(v.call("user32.dll", "PeekMessageW", &[msg, 0, 0, 0, 1]), 1);
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x401);
    assert_eq!(v.call("user32.dll", "SetTimer", &[hwnd, 7, 50, 0]), 7);
    assert_eq!(v.call("user32.dll", "SetTimer", &[hwnd, 1, 50, 0]), 1);
    let automatic = v.call("user32.dll", "SetTimer", &[hwnd, 0, 50, 0]);
    assert_ne!(automatic, 1);
    v.call("user32.dll", "KillTimer", &[hwnd, automatic]);
    v.call("user32.dll", "KillTimer", &[hwnd, 1]);
    v.state
        .user32
        .timers
        .get_mut(&(1, hwnd, 7))
        .unwrap()
        .deadline = std::time::Instant::now();
    assert_eq!(
        v.call("user32.dll", "PeekMessageW", &[msg, hwnd, 0, 0, 0]),
        1
    );
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x113);
    assert_eq!(v.mem.read_u32(msg + 8).unwrap(), 7);
    v.call("user32.dll", "KillTimer", &[hwnd, 7]);
    assert_eq!(
        v.call("user32.dll", "PeekMessageW", &[msg, hwnd, 0, 0, 1]),
        1
    );
    assert_eq!(v.call("user32.dll", "DispatchMessageW", &[msg]), 0);
    let mut proc = vec![0x8b, 0x44, 0x24, 12, 0xa3];
    proc.extend((DATA + 400).to_le_bytes());
    proc.extend([0x8b, 0x44, 0x24, 16, 0xa3]);
    proc.extend((DATA + 404).to_le_bytes());
    proc.extend([0x31, 0xc0, 0xc2, 16, 0]);
    v.mem.write_bytes(CODE, &proc).unwrap();
    v.call("user32.dll", "SetTimer", &[hwnd, 9, 50, CODE]);
    v.state
        .user32
        .timers
        .get_mut(&(1, hwnd, 9))
        .unwrap()
        .deadline = std::time::Instant::now();
    v.call("user32.dll", "PeekMessageW", &[msg, hwnd, 0, 0, 1]);
    let time = v.mem.read_u32(msg + 16).unwrap();
    v.call("user32.dll", "DispatchMessageW", &[msg]);
    assert_eq!(v.mem.read_u32(DATA + 400).unwrap(), 9);
    assert_eq!(v.mem.read_u32(DATA + 404).unwrap(), time);
    v.call("user32.dll", "PostQuitMessage", &[17]);
    assert_eq!(
        v.call("user32.dll", "GetMessageW", &[msg, hwnd, 0x900, 0x900]),
        0
    );
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x12);
    assert_eq!(v.mem.read_u32(msg + 8).unwrap(), 17);
    v.call("user32.dll", "DestroyWindow", &[hwnd]);
    assert!(v.state.user32.timers.is_empty());
    assert_eq!(
        v.call("user32.dll", "GetMessageW", &[msg, hwnd, 0, 0]),
        u32::MAX
    );
}

#[test]
fn blocked_message_thread_resumes_after_worker_posts_without_fake_quit() {
    let mut v = Vm::new();
    let post = v
        .api
        .register("user32.dll", Some("PostThreadMessageW"), None);
    let exit = v.api.register("kernel32.dll", Some("ExitThread"), None);
    let mut worker = vec![0x6a, 44, 0x6a, 33, 0x68];
    worker.extend(0x501u32.to_le_bytes());
    worker.extend([0x6a, 1, 0xb8]);
    worker.extend(post.to_le_bytes());
    worker.extend([0xff, 0xd0, 0x6a, 0, 0xb8]);
    worker.extend(exit.to_le_bytes());
    worker.extend([0xff, 0xd0, 0xcc]);
    v.mem.write_bytes(CODE, &worker).unwrap();
    v.k("CreateThread", &[0, 0, CODE, 0, 0, DATA]);
    let get = v.api.register("user32.dll", Some("GetMessageW"), None);
    let msg = DATA + 512;
    v.cpu.eip = get;
    let sp = v.cpu.esp();
    v.mem.write_u32(sp, RET).unwrap();
    for (i, n) in [msg, 0, 0, 0].into_iter().enumerate() {
        v.mem.write_u32(sp + 4 + i as u32 * 4, n).unwrap();
    }
    v.mem.write_u8(RET, 0xcc).unwrap();
    let outcome = v.cpu.run(
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        1_000_000,
    );
    assert!(matches!(outcome, wire::cpu::RunOutcome::Crashed(ref e) if e.contains("breakpoint")));
    assert_eq!(v.sched.current_tid(), 1);
    assert_eq!(v.cpu.regs[0], 1);
    assert_eq!(v.mem.read_u32(msg + 4).unwrap(), 0x501);
    assert_eq!(v.mem.read_u32(msg + 8).unwrap(), 33);
    assert_eq!(v.mem.read_u32(msg + 12).unwrap(), 44);
}

#[test]
fn cross_thread_sends_wake_when_window_is_destroyed_or_owner_exits() {
    fn call(code: &mut Vec<u8>, stub: u32, args: &[u32]) {
        for n in args.iter().rev() {
            code.push(0x68);
            code.extend(n.to_le_bytes());
        }
        code.push(0xb8);
        code.extend(stub.to_le_bytes());
        code.extend([0xff, 0xd0]);
    }
    for owner_exits in [false, true] {
        let mut v = Vm::new();
        v.mem
            .write_bytes(CODE + 0x600, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
            .unwrap();
        let hwnd = test_window(&mut v, CODE + 0x600);
        let send = v.api.register("user32.dll", Some("SendMessageW"), None);
        let exit = v.api.register("kernel32.dll", Some("ExitProcess"), None);
        let sleep = v.api.register("kernel32.dll", Some("Sleep"), None);
        let finish = v.api.register(
            if owner_exits {
                "kernel32.dll"
            } else {
                "user32.dll"
            },
            Some(if owner_exits {
                "ExitThread"
            } else {
                "DestroyWindow"
            }),
            None,
        );
        v.mem.write_u32(DATA + 0x7000, 0xdeadbeef).unwrap();
        let mut worker = Vec::new();
        call(&mut worker, send, &[hwnd, 0x405, 0, 0]);
        worker.push(0xa3);
        worker.extend((DATA + 0x7000).to_le_bytes());
        call(&mut worker, exit, &[0]);
        v.mem.write_bytes(CODE + 0x400, &worker).unwrap();
        v.k("CreateThread", &[0, 0, CODE + 0x400, 0, 0, 0]);
        let mut main = Vec::new();
        call(&mut main, sleep, &[1]);
        call(&mut main, finish, &[if owner_exits { 42 } else { hwnd }]);
        call(&mut main, sleep, &[100]);
        v.mem.write_bytes(CODE, &main).unwrap();
        v.cpu.eip = CODE;
        let result = v.cpu.run(
            &mut v.mem,
            &mut v.api,
            &mut v.state,
            &mut v.log,
            &mut v.sched,
            10000,
        );
        if let wire::cpu::RunOutcome::Crashed(error) = result {
            panic!("{error}");
        }
        assert!(matches!(result, wire::cpu::RunOutcome::Exited(0)));
        assert_eq!(v.mem.read_u32(DATA + 0x7000).unwrap(), 0);
        assert!(!v.state.user32.windows.contains_key(&hwnd));
    }
}

#[test]
fn cross_thread_send_message_runs_on_owner_teb_and_preserves_sender_result() {
    fn call(code: &mut Vec<u8>, stub: u32, args: &[u32]) {
        for &n in args.iter().rev() {
            code.push(0x68);
            code.extend(n.to_le_bytes());
        }
        code.push(0xb8);
        code.extend(stub.to_le_bytes());
        code.extend([0xff, 0xd0]);
    }
    let mut v = Vm::new();
    let mut wndproc = vec![0x64, 0xa1, 0x24, 0, 0, 0, 0xa3];
    wndproc.extend((DATA + 0x7000).to_le_bytes());
    wndproc.extend([0xb8, 0x34, 0x12, 0, 0, 0xc2, 16, 0]);
    v.mem.write_bytes(CODE + 0x600, &wndproc).unwrap();
    let hwnd = test_window(&mut v, CODE + 0x600);
    v.mem.write_u32(DATA + 0x7000, 0).unwrap();
    let send = v.api.register("user32.dll", Some("SendMessageW"), None);
    let post = v
        .api
        .register("user32.dll", Some("PostThreadMessageW"), None);
    let main_tid = v.sched.current_tid();
    v.mem.write_u32(TEB_ADDR + 0x24, main_tid).unwrap();
    let mut worker = Vec::new();
    call(&mut worker, send, &[hwnd, 0x405, 0, 0]);
    worker.push(0xa3);
    worker.extend((DATA + 0x7004).to_le_bytes());
    call(&mut worker, post, &[main_tid, 0x406, 0, 0]);
    worker.extend([0xc2, 4, 0]);
    v.mem.write_bytes(CODE + 0x400, &worker).unwrap();
    v.k("CreateThread", &[0, 0, CODE + 0x400, 0, 0, 0]);
    let get = v.api.register("user32.dll", Some("GetMessageW"), None);
    let exit = v.api.register("kernel32.dll", Some("ExitProcess"), None);
    let mut main = Vec::new();
    call(&mut main, get, &[DATA + 0x7100, 0, 0, 0]);
    call(&mut main, exit, &[0]);
    v.mem.write_bytes(CODE, &main).unwrap();
    v.cpu.eip = CODE;
    let result = v.cpu.run(
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        10000,
    );
    match result {
        wire::cpu::RunOutcome::Exited(0) => {}
        wire::cpu::RunOutcome::Crashed(e) => panic!("{e}"),
        _ => panic!("did not exit"),
    }
    assert_eq!(v.mem.read_u32(DATA + 0x7000).unwrap(), main_tid);
    assert_eq!(v.mem.read_u32(DATA + 0x7004).unwrap(), 0x1234);
    assert_eq!(v.mem.read_u32(DATA + 0x7104).unwrap(), 0x406);
}

#[test]
fn window_enumeration_filters_threads_and_children_stops_on_false_and_skips_destroyed_windows() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
        .unwrap();
    let first = test_window(&mut v, CODE);
    let atom = v.state.user32.windows[&first].class as u32;
    let second = v.call(
        "user32.dll",
        "CreateWindowExW",
        &[0, atom, 0, 0, 0, 0, 100, 100, 0, 0, 0x400000, 0],
    );
    let child = v.call(
        "user32.dll",
        "CreateWindowExW",
        &[0, atom, 0, 0x40000000, 0, 0, 10, 10, first, 0, 0x400000, 0],
    );
    assert_ne!(child, 0);
    let callback = CODE + 0x400;
    v.mem
        .write_bytes(
            callback,
            &[
                0x8b, 0x54, 0x24, 8, 0x8b, 0x0a, 0x8b, 0x44, 0x24, 4, 0x89, 0x44, 0x8a, 4, 0xff, 2,
                0xb8, 1, 0, 0, 0, 0xc2, 8, 0,
            ],
        )
        .unwrap();
    assert_eq!(
        v.call("user32.dll", "EnumWindows", &[callback, DATA + 0x500]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0x500).unwrap(), 2);
    assert_eq!(v.mem.read_u32(DATA + 0x504).unwrap(), second);
    assert_eq!(v.mem.read_u32(DATA + 0x508).unwrap(), first);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetWindowThreadProcessId",
            &[first, DATA + 0x600]
        ),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0x600).unwrap(), 4242);
    v.mem.write_u32(DATA + 0x500, 0).unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "EnumThreadWindows",
            &[999, callback, DATA + 0x500]
        ),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0x500).unwrap(), 0);

    let set_error = v.api.register("kernel32.dll", Some("SetLastError"), None);
    let mut stop = vec![0x68];
    stop.extend(777u32.to_le_bytes());
    stop.push(0xb8);
    stop.extend(set_error.to_le_bytes());
    stop.extend([0xff, 0xd0, 0x31, 0xc0, 0xc2, 8, 0]);
    v.mem.write_bytes(callback, &stop).unwrap();
    assert_eq!(
        v.call("user32.dll", "EnumThreadWindows", &[1, callback, 0]),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 777);
    assert_eq!(v.cpu.eip, RET);
    let destroy = v.api.register("user32.dll", Some("DestroyWindow"), None);
    let mut code = vec![0xff, 5];
    code.extend((DATA + 0x500).to_le_bytes());
    code.push(0x68);
    code.extend(first.to_le_bytes());
    code.push(0xb8);
    code.extend(destroy.to_le_bytes());
    code.extend([0xff, 0xd0, 0xb8, 1, 0, 0, 0, 0xc2, 8, 0]);
    v.mem.write_bytes(callback, &code).unwrap();
    assert_eq!(v.call("user32.dll", "EnumWindows", &[callback, 0]), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x500).unwrap(), 1); // second's callback destroyed first in the snapshot
    assert!(!v.state.user32.windows.contains_key(&first));

    // An exiting callback retains its new CPU context and ends enumeration.
    assert_ne!(
        v.call(
            "user32.dll",
            "CreateWindowExW",
            &[0, atom, 0, 0, 0, 0, 100, 100, 0, 0, 0x400000, 0]
        ),
        0
    );
    v.mem.write_u32(DATA + 0x500, 0).unwrap();
    let exit = v.api.register("kernel32.dll", Some("ExitProcess"), None);
    let mut code = vec![0xff, 5];
    code.extend((DATA + 0x500).to_le_bytes());
    code.extend([0x68, 37, 0, 0, 0, 0xb8]);
    code.extend(exit.to_le_bytes());
    code.extend([0xff, 0xd0, 0xc2, 8, 0]);
    v.mem.write_bytes(callback, &code).unwrap();
    v.call("user32.dll", "EnumWindows", &[callback, 0]);
    assert_eq!(v.mem.read_u32(DATA + 0x500).unwrap(), 1);
    assert_eq!(v.cpu.exit_code, Some(37));
    assert_eq!(v.cpu.eip, exit);
}

#[test]
fn message_object_waits_preserve_queue_masks_old_input_and_atomic_acquisition() {
    let mut v = Vm::new();
    let tid = v.sched.current_tid();
    let event = v.k("CreateEventW", &[0, 0, 0, 0]);
    v.mem.write_u32(DATA, event).unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjects",
            &[1, DATA, 0, 0, 8]
        ),
        0x102
    );
    assert_eq!(
        v.call("user32.dll", "PostThreadMessageW", &[tid, 0x400, 0, 0]),
        1
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjects",
            &[1, DATA, 0, 0, 8]
        ),
        1
    );
    assert_eq!(
        v.call("user32.dll", "PeekMessageW", &[DATA + 0x100, 0, 0, 0, 0]),
        1
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjects",
            &[1, DATA, 0, 0, 8]
        ),
        0x102
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjectsEx",
            &[1, DATA, 0, 8, 4]
        ),
        1
    );
    assert_eq!(v.k("SetEvent", &[event]), 1);
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjectsEx",
            &[1, DATA, 0, 8, 5]
        ),
        0
    );
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 0x102);
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjects",
            &[64, DATA, 0, 0, 8]
        ),
        u32::MAX
    );
    assert_eq!(v.state.last_error, 87);
    v.mem.write_u32(DATA, 0xdeadbeef).unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjects",
            &[1, DATA, 0, 0, 8]
        ),
        u32::MAX
    );
    assert_eq!(v.state.last_error, 6);
}

#[test]
fn validated_paint_drops_only_synthetic_messages_and_default_close_destroys_window() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
        .unwrap();
    let hwnd = test_window(&mut v, CODE);
    v.call("user32.dll", "ShowWindow", &[hwnd, 5]);
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0xf, 0xf, 0]
        ),
        1
    );
    v.call("user32.dll", "ValidateRect", &[hwnd, 0]);
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0xf, 0xf, 0]
        ),
        0
    );
    v.call("user32.dll", "PostMessageW", &[hwnd, 0xf, 0, 0]);
    v.call("user32.dll", "ValidateRect", &[hwnd, 0]);
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0xf, 0xf, 1]
        ),
        1
    );
    assert_eq!(
        v.call("user32.dll", "DefWindowProcW", &[hwnd, 0x10, 0, 0]),
        1
    );
    assert_eq!(v.call("user32.dll", "IsWindow", &[hwnd]), 0);
}
