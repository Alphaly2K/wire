//! Independent baseline: window queries, field widths and coordinate failures.
use super::*;

fn window(v: &mut Vm) -> u32 {
    let proc = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    test_window(v, proc)
}

#[test]
fn window_queries_contract_pe32_keeps_invalid_handles_and_abi_policies() {
    let mut v = Vm::new();
    v.k("SetLastError", &[77]);
    for name in [
        "IsWindow",
        "IsWindowVisible",
        "IsWindowUnicode",
        "IsIconic",
        "IsZoomed",
        "IsWindowEnabled",
    ] {
        assert_eq!(v.call("user32.dll", name, &[1]), 0, "{name}");
        assert_eq!(v.state.last_error, 77, "{name}");
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 8);
    }
    for name in [
        "GetParent",
        "GetClientRect",
        "GetWindowRect",
        "GetWindowThreadProcessId",
    ] {
        assert_eq!(v.call("user32.dll", name, &[1, 0x30000]), 0, "{name}");
        assert_eq!(v.state.last_error, 1400);
    }
    assert_eq!(v.call("user32.dll", "GetWindowPlacement", &[1, 0x30000]), 0);
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 12);
    for name in [
        "GetWindowLongPtrA",
        "GetWindowLongPtrW",
        "SetWindowLongPtrA",
        "SetWindowLongPtrW",
    ] {
        assert_eq!(v.call("user32.dll", name, &[1, 0, 42]), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
    }
    assert_eq!(v.call("kernel32.dll", "IsWindow", &[1]), 0);
    assert_eq!(v.state.last_error, 120);
}

#[test]
fn window_queries_contract_pe32_keeps_fields_extra_storage_and_partial_rect_output() {
    let mut v = Vm::new();
    let hwnd = window(&mut v);
    for (index, value) in [
        (-6, 0xfedcba98),
        (-12, 0x89abcdef),
        (-21, 0xffffffff),
        (0, 0x12345678),
        (4, 0xaabbccdd),
    ] {
        v.call("user32.dll", "SetWindowLongA", &[hwnd, index as u32, value]);
        assert_eq!(
            v.call("user32.dll", "GetWindowLongW", &[hwnd, index as u32]),
            value
        );
    }
    assert_eq!(
        v.call("user32.dll", "GetWindowLongA", &[hwnd, 2]),
        0xccdd1234
    );
    assert_eq!(v.call("user32.dll", "GetWindowLongW", &[hwnd, 5]), 0);
    assert_eq!(v.state.last_error, 1413);
    assert_eq!(
        v.call("user32.dll", "SetWindowLongW", &[1, (-4i32) as u32, 0]),
        0
    );
    assert_eq!(v.state.last_error, 1400);
    assert_eq!(
        v.call("user32.dll", "SetWindowLongW", &[hwnd, (-4i32) as u32, 0]),
        0
    );
    assert_eq!(v.state.last_error, 87);
    // PE32 preserves the unvalidated nonzero WNDPROC and exact old value.
    let old = v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-4i32) as u32, 0x30000],
    );
    assert_ne!(old, 0);
    assert_eq!(
        v.call("user32.dll", "GetWindowLongA", &[hwnd, (-4i32) as u32]),
        0x30000
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "GetWindowThreadProcessId",
            &[hwnd, DATA + 512]
        ),
        v.state.user32.windows[&hwnd].tid
    );
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 4242);
    v.k("SetLastError", &[77]);
    let edge = DATA + 0x10000 - 8;
    v.mem.write_bytes(edge, &[0xaa; 8]).unwrap();
    assert!(v
        .try_call("user32.dll", "GetWindowRect", &[hwnd, edge])
        .is_err());
    assert_eq!(v.mem.read_u32(edge).unwrap(), 3);
    assert_eq!(v.mem.read_u32(edge + 4).unwrap(), 4);
    assert_eq!(v.state.last_error, 77);
}

#[test]
fn window_queries_contract_pe32_keeps_coordinate_validation_and_cycles() {
    let mut v = Vm::new();
    let hwnd = window(&mut v);
    v.mem.write_u32(DATA + 512, 10).unwrap();
    v.mem.write_u32(DATA + 516, 20).unwrap();
    assert_eq!(
        v.call("user32.dll", "ClientToScreen", &[hwnd, DATA + 512]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 13);
    assert_eq!(v.mem.read_u32(DATA + 516).unwrap(), 24);
    assert_eq!(
        v.call("user32.dll", "ScreenToClient", &[hwnd, DATA + 512]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 10);
    assert_eq!(
        v.call("user32.dll", "MapWindowPoints", &[hwnd, 0, DATA + 512, 1]),
        0x00040003
    );
    assert_eq!(v.call("user32.dll", "ClientToScreen", &[1, 0x30000]), 0);
    assert_eq!(v.state.last_error, 1400);
    assert_eq!(v.call("user32.dll", "ClientToScreen", &[hwnd, 0x30000]), 0);
    assert_eq!(v.state.last_error, 87);
    assert_eq!(
        v.call("user32.dll", "MapWindowPoints", &[1, 1, 0x30000, 4097]),
        0
    );
    assert_eq!(v.state.last_error, 87);
    assert!(v
        .try_call("user32.dll", "MapWindowPoints", &[1, 0, DATA + 512, 0])
        .is_err());
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-16i32) as u32, 0x40000000],
    );
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-8i32) as u32, hwnd],
    );
    assert!(v
        .try_call("user32.dll", "GetWindowRect", &[hwnd, DATA + 512])
        .unwrap_err()
        .contains("cyclic parent"));
    assert_eq!(
        v.call("user32.dll", "ClientToScreen", &[hwnd, DATA + 512]),
        0
    );
    assert_eq!(v.state.last_error, 1400);
}

#[test]
fn window_queries_contract_pe32_keeps_desktop_find_and_top_level_filter() {
    let mut v = Vm::new();
    let hwnd = window(&mut v);
    v.wide(DATA + 512, "CALLBACKWINDOW");
    assert_eq!(v.call("user32.dll", "FindWindowW", &[DATA + 512, 0]), hwnd);
    let atom = v.state.user32.windows[&hwnd].class as u32;
    assert_eq!(v.call("user32.dll", "FindWindowA", &[atom, 0]), hwnd);
    let desktop = v.call("user32.dll", "GetDesktopWindow", &[]);
    assert_eq!(desktop, 0x4000fffc);
    assert_eq!(v.call("user32.dll", "GetDesktopWindow", &[]), desktop);
    assert_eq!(v.state.user32.windows.len(), 2);
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-16i32) as u32, 0x40000000],
    );
    assert_eq!(v.call("user32.dll", "FindWindowW", &[atom, 0]), 0);
    assert!(v
        .try_call("user32.dll", "FindWindowW", &[0x30000, 0x30000])
        .unwrap_err()
        .starts_with("FindWindow class:"));
}
