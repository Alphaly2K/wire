//! Independent baseline for text callbacks and input ownership commit order.
use super::*;

fn window(v: &mut Vm, wide: bool) -> u32 {
    let procedure = v.api.register(
        "user32.dll",
        Some(if wide {
            "DefWindowProcW"
        } else {
            "DefWindowProcA"
        }),
        None,
    );
    test_window(v, procedure)
}

#[test]
fn window_state_contract_pe32_keeps_text_callback_encoding_and_capacity_rules() {
    for wide in [false, true] {
        let mut v = Vm::new();
        let hwnd = window(&mut v, wide);
        let suffix = if wide { 'W' } else { 'A' };
        let text = DATA + 512;
        let output = DATA + 768;
        if wide {
            v.wide(text, "日本A");
        } else {
            v.mem
                .write_bytes(text, &[0x93, 0xfa, 0x96, 0x7b, b'A', 0])
                .unwrap();
        }
        v.k("SetLastError", &[77]);
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("SetWindowText{suffix}"),
                &[hwnd, text]
            ),
            1
        );
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("GetWindowTextLength{suffix}"),
                &[hwnd]
            ),
            if wide { 3 } else { 5 }
        );
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("GetWindowText{suffix}"),
                &[hwnd, output, 2]
            ),
            1
        );
        if wide {
            assert_eq!(v.mem.read_u32(output).unwrap(), 0x65e5);
        } else {
            assert_eq!(v.mem.read_u16(output).unwrap(), 0x0093);
        }
        for (pointer, count) in [(0, 1), (0x30000, 0), (0x30000, u32::MAX)] {
            assert_eq!(
                v.call(
                    "user32.dll",
                    &format!("GetWindowText{suffix}"),
                    &[hwnd, pointer, count]
                ),
                0
            );
        }
        assert_eq!(v.state.last_error, 77);
        assert_eq!(
            v.call("user32.dll", &format!("SetWindowText{suffix}"), &[hwnd, 0]),
            1
        );
        assert_eq!(
            v.call(
                "user32.dll",
                &format!("GetWindowTextLength{suffix}"),
                &[hwnd]
            ),
            0
        );
    }
}

#[test]
fn window_state_contract_pe32_keeps_hwnd_thread_and_fault_validation_order() {
    let mut v = Vm::new();
    let hwnd = window(&mut v, true);
    assert_eq!(v.call("user32.dll", "SetWindowTextW", &[1, 0x30000]), 0);
    assert_eq!(v.state.last_error, 1400);
    v.state.user32.windows.get_mut(&hwnd).unwrap().tid += 1;
    assert_eq!(v.call("user32.dll", "SetWindowTextW", &[hwnd, 0x30000]), 0);
    assert_eq!(v.state.last_error, 120);
    v.state.user32.windows.get_mut(&hwnd).unwrap().tid -= 1;
    v.wide(DATA + 512, "still here");
    v.call("user32.dll", "SetWindowTextW", &[hwnd, DATA + 512]);
    assert!(v
        .try_call("user32.dll", "SetWindowTextW", &[hwnd, 0x30000])
        .is_err());
    assert_eq!(v.call("user32.dll", "GetWindowTextLengthW", &[hwnd]), 10);
    v.k("SetLastError", &[77]);
    assert_eq!(v.call("user32.dll", "GetLastActivePopup", &[1]), 1);
    assert_eq!(v.state.last_error, 77);
    for name in [
        "SetFocus",
        "SetActiveWindow",
        "SetForegroundWindow",
        "SetCapture",
    ] {
        assert_eq!(v.call("user32.dll", name, &[1]), 0);
        assert_eq!(v.state.last_error, 1400);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 8);
    }
}

#[test]
fn window_state_contract_pe32_keeps_reentrant_callback_order_and_null_clears() {
    let mut v = Vm::new();
    let count = DATA + 0x800;
    let focus = v.api.register("user32.dll", Some("GetFocus"), None);
    let mut procedure = vec![0x8b, 0x15];
    procedure.extend(count.to_le_bytes());
    for (field, stack) in [4, 8, 12, 16].into_iter().enumerate() {
        procedure.extend([0x8b, 0x44, 0x24, stack, 0x89, 0x04, 0x95]);
        procedure.extend((DATA + 0x1000 + field as u32 * 0x100).to_le_bytes());
    }
    procedure.extend([0x52, 0xb8]);
    procedure.extend(focus.to_le_bytes());
    procedure.extend([0xff, 0xd0, 0x5a, 0x89, 0x04, 0x95]);
    procedure.extend((DATA + 0x1400).to_le_bytes());
    procedure.extend([0x42, 0x89, 0x15]);
    procedure.extend(count.to_le_bytes());
    procedure.extend([
        0x8b, 0x44, 0x24, 8, 0x3d, 0x81, 0, 0, 0, 0x0f, 0x94, 0xc0, 0x0f, 0xb6, 0xc0, 0xc2, 16, 0,
    ]);
    v.mem.write_bytes(CODE, &procedure).unwrap();
    let hwnd = test_window(&mut v, CODE);
    v.mem.write_u32(count, 0).unwrap();
    assert_eq!(v.call("user32.dll", "SetFocus", &[hwnd]), 0);
    assert_eq!(v.call("user32.dll", "SetFocus", &[hwnd]), hwnd);
    assert_eq!(v.call("user32.dll", "SetFocus", &[0]), hwnd);
    assert_eq!(v.call("user32.dll", "SetCapture", &[hwnd]), 0);
    assert_eq!(v.call("user32.dll", "ReleaseCapture", &[]), 1);
    assert_eq!(v.call("user32.dll", "SetActiveWindow", &[hwnd]), 0);
    assert_eq!(v.call("user32.dll", "SetActiveWindow", &[hwnd]), hwnd);
    assert_eq!(v.call("user32.dll", "SetForegroundWindow", &[0]), 1);
    assert_eq!(v.mem.read_u32(count).unwrap(), 5);
    for (i, (message, observed_focus)) in [(7, hwnd), (8, 0), (0x215, 0), (6, 0), (6, 0)]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            v.mem.read_u32(DATA + 0x1100 + i as u32 * 4).unwrap(),
            message
        );
        assert_eq!(
            v.mem.read_u32(DATA + 0x1400 + i as u32 * 4).unwrap(),
            observed_focus
        );
    }
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
fn window_state_contract_pe32_keeps_state_committed_when_callback_faults() {
    let mut v = Vm::new();
    let hwnd = window(&mut v, true);
    v.mem.write_bytes(CODE, &[0x0f, 0x0b]).unwrap();
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-4i32) as u32, CODE],
    );
    assert!(v.try_call("user32.dll", "SetFocus", &[hwnd]).is_err());
    assert_eq!(v.call("user32.dll", "GetFocus", &[]), hwnd);
}

#[test]
fn window_state_reloads_target_after_previous_notification_replaces_or_destroys_it() {
    for destroy in [false, true] {
        let mut v = Vm::new();
        let first = window(&mut v, true);
        let class = v.state.user32.windows[&first].class as u32;
        let second = v.call(
            "user32.dll",
            "CreateWindowExW",
            &[0, class, 0, 0, 3, 4, 640, 480, 0, 0, 0x400000, 0],
        );
        assert_ne!(second, 0);
        let old_count = DATA + 0x2000;
        let new_count = DATA + 0x2004;
        for (address, counter) in [(CODE + 0x200, old_count), (CODE + 0x300, new_count)] {
            // Count WM_SETFOCUS only; window destruction has its own callbacks.
            let mut procedure = vec![0x83, 0x7c, 0x24, 8, 7, 0x75, 6, 0xff, 0x05];
            procedure.extend(counter.to_le_bytes());
            procedure.extend([0xb8, 1, 0, 0, 0, 0xc2, 16, 0]);
            v.mem.write_bytes(address, &procedure).unwrap();
        }
        v.call(
            "user32.dll",
            "SetWindowLongW",
            &[second, u32::MAX - 3, CODE + 0x200],
        );
        let mut action = Vec::new();
        let gateway = if destroy {
            v.api.register("user32.dll", Some("DestroyWindow"), None)
        } else {
            action.push(0x68);
            action.extend((CODE + 0x300).to_le_bytes());
            action.push(0x68);
            action.extend((-4i32).to_le_bytes());
            v.api.register("user32.dll", Some("SetWindowLongW"), None)
        };
        action.push(0x68);
        action.extend(second.to_le_bytes());
        action.push(0xb8);
        action.extend(gateway.to_le_bytes());
        action.extend([0xff, 0xd0]);
        let mut procedure = vec![0x83, 0x7c, 0x24, 8, 8, 0x75, action.len() as u8];
        procedure.extend(action);
        procedure.extend([0xb8, 1, 0, 0, 0, 0xc2, 16, 0]);
        v.mem.write_bytes(CODE, &procedure).unwrap();
        v.call("user32.dll", "SetWindowLongW", &[first, u32::MAX - 3, CODE]);
        assert_eq!(v.call("user32.dll", "SetFocus", &[first]), 0);
        assert_eq!(v.call("user32.dll", "SetFocus", &[second]), first);
        assert_eq!(v.mem.read_u32(old_count).unwrap(), 0);
        assert_eq!(v.mem.read_u32(new_count).unwrap(), (!destroy) as u32);
        assert_eq!(
            v.call("user32.dll", "GetFocus", &[]),
            if destroy { 0 } else { second }
        );
        assert_eq!(v.state.user32.windows.contains_key(&second), !destroy);
    }
}
