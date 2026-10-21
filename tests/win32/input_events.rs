use super::*;

fn window(v: &mut Vm) -> u32 {
    v.mem
        .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
        .unwrap();
    test_window(v, CODE)
}

#[test]
fn input_events_pe32_native_activation_updates_owners_and_delivers_notifications() {
    let mut v = Vm::new();
    let hwnd = window(&mut v);
    for (message, wp, query, expected) in [
        (0x1c, 1, "GetForegroundWindow", hwnd),
        (6, 1, "GetActiveWindow", hwnd),
        (7, 0, "GetFocus", hwnd),
        (8, 0, "GetFocus", 0),
        (6, 0, "GetActiveWindow", 0),
        (0x1c, 0, "GetForegroundWindow", 0),
    ] {
        wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, message, wp, 0);
        assert_eq!(v.call("user32.dll", query, &[]), expected);
        assert_eq!(
            v.call(
                "user32.dll",
                "PeekMessageW",
                &[DATA + 0x900, hwnd, message, message, 1]
            ),
            1
        );
        assert_eq!(v.mem.read_u32(DATA + 0x904).unwrap(), message);
        assert_eq!(v.mem.read_u32(DATA + 0x908).unwrap(), wp);
    }
}

#[test]
fn input_events_pe32_modifier_sides_and_all_buttons_commit_only_when_removed() {
    let mut v = Vm::new();
    let hwnd = window(&mut v);
    for (message, key, lp, specific, generic, held) in [
        (0x100, 16, 0x002a0001, 0xa0, 16, true),
        (0x100, 16, 0x00360001, 0xa1, 16, true),
        (0x101, 16, 0xc02a0001, 0xa0, 16, true),
        (0x101, 16, 0xc0360001, 0xa1, 16, false),
        (0x100, 17, 0x011d0001, 0xa3, 17, true),
        (0x101, 17, 0xc11d0001, 0xa3, 17, false),
        (0x104, 18, 0x01380001, 0xa5, 18, true),
        (0x105, 18, 0xe1380001, 0xa5, 18, false),
        (0x201, 1, 0, 1, 1, true),
        (0x202, 0, 0, 1, 1, false),
        (0x204, 2, 0, 2, 2, true),
        (0x205, 0, 0, 2, 2, false),
        (0x207, 16, 0, 4, 4, true),
        (0x208, 0, 0, 4, 4, false),
        (0x20b, 0x10020, 0, 5, 5, true),
        (0x20c, 0x10000, 0, 5, 5, false),
        (0x20b, 0x20040, 0, 6, 6, true),
        (0x20c, 0x20000, 0, 6, 6, false),
    ] {
        let before = v.call("user32.dll", "GetKeyState", &[specific]);
        wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, message, key, lp);
        let async_state = v.call("user32.dll", "GetAsyncKeyState", &[generic]);
        assert_eq!(async_state & 0x8000 != 0, held);
        assert_eq!(
            v.call(
                "user32.dll",
                "PeekMessageW",
                &[DATA + 0x900, hwnd, message, message, 0]
            ),
            1
        );
        assert_eq!(v.call("user32.dll", "GetKeyState", &[specific]), before);
        assert_eq!(
            v.call(
                "user32.dll",
                "PeekMessageW",
                &[DATA + 0x900, hwnd, message, message, 1]
            ),
            1
        );
        assert_eq!(v.mem.read_u32(DATA + 0x908).unwrap(), key);
        assert_eq!(v.mem.read_u32(DATA + 0x90c).unwrap(), lp);
        assert_eq!(
            v.call("user32.dll", "GetKeyState", &[generic]) & 0x8000 != 0,
            held
        );
        assert_eq!(
            v.call("user32.dll", "GetKeyState", &[specific]) & 0x8000 != 0,
            matches!(message, 0x100 | 0x104 | 0x201 | 0x204 | 0x207 | 0x20b)
        );
    }
    // A repeated key-down keeps the high bit without reporting another press.
    wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x100, 65, 0x1e0001);
    assert_eq!(v.call("user32.dll", "GetAsyncKeyState", &[65]), 0xffff8001);
    wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x100, 65, 0x401e0001);
    assert_eq!(v.call("user32.dll", "GetAsyncKeyState", &[65]), 0xffff8000);
}

#[test]
fn input_events_pe32_wheels_use_guest_screen_points_and_system_text_preserves_surrogates() {
    let mut v = Vm::new();
    let hwnd = window(&mut v);
    let local = 0x0019fffe;
    let screen = 1 | (29 << 16); // client origin (3,4) plus (-2,25).
    for (message, wp, expected) in [
        (0x200, 12, local),
        (0x20a, 0xff10000c, screen),
        (0x20e, 0x78000c, screen),
    ] {
        wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, message, wp, local);
        wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x200, 0, 0);
        assert_eq!(
            v.call(
                "user32.dll",
                "GetMessageW",
                &[DATA + 0x900, hwnd, message, message]
            ),
            1
        );
        assert_eq!(v.mem.read_u32(DATA + 0x908).unwrap(), wp);
        assert_eq!(v.mem.read_u32(DATA + 0x90c).unwrap(), expected);
        assert_eq!(v.mem.read_u32(DATA + 0x914).unwrap(), 1);
        assert_eq!(v.mem.read_u32(DATA + 0x918).unwrap(), 29);
        while v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0x200, 0x200, 1],
        ) != 0
        {}
    }
    let lp = 0x201e0001;
    wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x104, 65, lp);
    for c in [0x3042, 0xd83d, 0xde42] {
        wire::user32::host_message(&mut v.state, &mut v.sched, hwnd, 0x106, c, lp);
    }
    assert_eq!(
        v.call(
            "user32.dll",
            "PeekMessageW",
            &[DATA + 0x900, hwnd, 0x106, 0x106, 1]
        ),
        0
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "GetMessageW",
            &[DATA + 0x900, hwnd, 0x104, 0x104]
        ),
        1
    );
    assert_eq!(v.call("user32.dll", "TranslateMessage", &[DATA + 0x900]), 1);
    for c in [0x3042, 0xd83d, 0xde42] {
        assert_eq!(
            v.call(
                "user32.dll",
                "GetMessageW",
                &[DATA + 0x900, hwnd, 0x106, 0x106]
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
            &[DATA + 0x900, hwnd, 0x106, 0x106, 1]
        ),
        0
    );
}
