use super::*;
use wire::x64::process::{Outcome, Process64};
use wire::x64::runtime::WindowEvent;

const DATA: u64 = 0x6c0001000;

fn window() -> (Process64, u64) {
    let path = std::env::temp_dir().join(format!(
        "wire-input64-{}-{:?}.exe",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, fixture()).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    std::fs::remove_file(path).unwrap();
    let entry = p.cpu.clone();
    p.memory
        .map(DATA - 4096, 8192, READ | WRITE | EXECUTE)
        .unwrap();
    p.memory
        .write(DATA - 4096, &[0xb8, 1, 0, 0, 0, 0xc3])
        .unwrap();
    p.memory
        .write(
            DATA,
            &"InputWindow\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    p.memory.write_int(DATA + 0x100, 4, 80).unwrap();
    p.memory.write_int(DATA + 0x108, 8, DATA - 4096).unwrap();
    p.memory.write_int(DATA + 0x118, 8, BASE).unwrap();
    p.memory.write_int(DATA + 0x140, 8, DATA).unwrap();
    assert_ne!(call(&mut p, "RegisterClassExW", &[DATA + 0x100]), 0);
    let gateway = p.runtime.register("user32.dll", "CreateWindowExW");
    let args = [0, DATA, 0, 0, 3, 4, 640, 480, 0, 0, BASE, 0];
    let mut code = vec![0x48, 0x83, 0xec, 0x68];
    for (i, arg) in args.into_iter().enumerate() {
        if i < 4 {
            code.extend([[0x48, 0xb9], [0x48, 0xba], [0x49, 0xb8], [0x49, 0xb9]][i]);
            code.extend(arg.to_le_bytes());
        } else {
            code.extend([0x48, 0xb8]);
            code.extend(arg.to_le_bytes());
            code.extend([0x48, 0x89, 0x44, 0x24, 32 + (i as u8 - 4) * 8]);
        }
    }
    code.extend([0x48, 0xb8]);
    code.extend(gateway.to_le_bytes());
    code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x68, 0xc3]);
    p.memory.write(BASE + 0x1000, &code).unwrap();
    p.cpu = entry;
    let hwnd = match p.run(1000).unwrap() {
        Outcome::Returned(hwnd) => hwnd,
        other => panic!("{other:?}"),
    };
    assert!(hwnd > u32::MAX as u64);
    (p, hwnd)
}
fn call(p: &mut Process64, name: &str, args: &[u64]) -> u64 {
    startup_dll_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "user32.dll",
        name,
        args,
    )
}
fn host(p: &mut Process64, hwnd: u64, message: u32, wparam: u32, lparam: u32) {
    p.runtime
        .queue_window_event(WindowEvent {
            window: hwnd,
            message,
            wparam: wparam as u64,
            lparam: lparam as i32 as i64 as u64,
        })
        .unwrap();
}
fn peek(p: &mut Process64, hwnd: u64, message: u32, remove: bool) -> u64 {
    call(
        p,
        "PeekMessageW",
        &[
            DATA + 0x900,
            hwnd,
            message as u64,
            message as u64,
            remove as u64,
        ],
    )
}

#[test]
fn input_events_pointer_mouse_snapshots_follow_dequeued_events_and_checked_output() {
    let (mut p, hwnd) = window();
    assert_eq!(call(&mut p, "IsMouseInPointerEnabled", &[]), 0);
    assert_eq!(call(&mut p, "EnableMouseInPointer", &[1]), 1);
    assert_eq!(call(&mut p, "IsMouseInPointerEnabled", &[]), 1);
    assert_eq!(call(&mut p, "EnableMouseInPointer", &[0]), 0);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        5
    );
    let out = DATA + 0xa00;
    assert_eq!(call(&mut p, "GetPointerType", &[1, out]), 0);
    host(&mut p, hwnd, 0x201, 1 | 4, (20 << 16) | 10);
    host(&mut p, hwnd, 0x202, 4, (40 << 16) | 30);
    assert_eq!(peek(&mut p, hwnd, 0x201, true), 0);
    assert_eq!(peek(&mut p, hwnd, 0x246, false), 1);
    assert_eq!(p.memory.read_int(DATA + 0x910, 8).unwrap(), 0x20170001);
    assert_eq!(p.memory.read_int(DATA + 0x918, 8).unwrap(), (24 << 16) | 13);
    assert_eq!(call(&mut p, "GetPointerType", &[1, out]), 1);
    assert_eq!(p.memory.read_int(out, 4).unwrap(), 4);
    assert_eq!(call(&mut p, "GetPointerInfo", &[1, out]), 1);
    assert_eq!(p.memory.read_int(out + 8, 4).unwrap(), 1);
    assert_eq!(p.memory.read_int(out + 12, 4).unwrap(), 0x12017);
    assert_eq!(p.memory.read_int(out + 24, 8).unwrap(), hwnd);
    assert_eq!(p.memory.read_int(out + 32, 8).unwrap(), (24 << 32) | 13);
    assert_eq!(p.memory.read_int(out + 76, 4).unwrap(), 4);
    assert_eq!(p.memory.read_int(out + 88, 4).unwrap(), 1);
    assert_eq!(call(&mut p, "GetKeyState", &[1]) & 0x8000, 0);
    assert_eq!(peek(&mut p, hwnd, 0x246, true), 1);
    assert_eq!(call(&mut p, "GetKeyState", &[1]) & 0x8000, 0x8000);
    assert_eq!(peek(&mut p, hwnd, 0x247, true), 1);
    assert_eq!(call(&mut p, "GetPointerInfo", &[1, out]), 1);
    assert_eq!(p.memory.read_int(out + 12, 4).unwrap(), 0x42002);
    assert_eq!(p.memory.read_int(out + 32, 8).unwrap(), (44 << 32) | 33);
    assert_eq!(p.memory.read_int(out + 88, 4).unwrap(), 2);
    assert_eq!(call(&mut p, "GetKeyState", &[1]) & 0x8000, 0);
    p.memory.write(out, &[0xaa; 96]).unwrap();
    assert_eq!(call(&mut p, "GetPointerInfo", &[2, out]), 0);
    let mut bytes = [0; 96];
    p.memory.read(out, &mut bytes).unwrap();
    assert_eq!(bytes, [0xaa; 96]);
    let crossing = DATA + 4096 - 48;
    p.memory.write(crossing, &[0xbb; 48]).unwrap();
    p.cpu.rip = p.runtime.register("user32.dll", "GetPointerInfo");
    p.cpu.regs[1] = 1;
    p.cpu.regs[2] = crossing;
    let before = p.cpu.clone();
    assert!(p
        .runtime
        .dispatch("user32.dll", "GetPointerInfo", &mut p.cpu, &mut p.memory)
        .is_err());
    assert_eq!(p.cpu.rip, before.rip);
    assert_eq!(p.cpu.regs, before.regs);
    let mut prefix = [0; 48];
    p.memory.read(crossing, &mut prefix).unwrap();
    assert_eq!(prefix, [0xbb; 48]);
}

#[test]
fn input_events_pointer_mouse_button_chords_and_wheels_keep_pointer_identity() {
    let (mut p, hwnd) = window();
    assert_eq!(call(&mut p, "EnableMouseInPointer", &[1]), 1);
    for (message, wp, promoted, flags, change) in [
        (0x201, 1, 0x246, 0x12017, 1),
        (0x204, 3, 0x245, 0x22036, 3),
        (0x202, 2, 0x245, 0x22026, 2),
        (0x205, 0, 0x247, 0x42002, 4),
        (0x207, 16, 0x246, 0x12046, 5),
        (0x208, 0, 0x247, 0x42002, 6),
        (0x20b, 0x10020, 0x246, 0x12086, 7),
        (0x20c, 0x10000, 0x247, 0x42002, 8),
        (0x20b, 0x20040, 0x246, 0x12106, 9),
        (0x20c, 0x20000, 0x247, 0x42002, 10),
        (0x20a, 0xff880000, 0x24e, 0x82002, 0),
        (0x20e, 0x00780000, 0x24f, 0x102002, 0),
    ] {
        host(&mut p, hwnd, message, wp, (70 << 16) | 60);
        assert_eq!(peek(&mut p, hwnd, promoted, true), 1);
        let actual_wp = p.memory.read_int(DATA + 0x910, 8).unwrap();
        assert_eq!(actual_wp & 0xffff, 1);
        if matches!(message, 0x20a | 0x20e) {
            assert_eq!(actual_wp as u32 & 0xffff0000, wp & 0xffff0000);
        }
        assert_eq!(call(&mut p, "GetPointerInfo", &[1, DATA + 0xa00]), 1);
        assert_eq!(p.memory.read_int(DATA + 0xa0c, 4).unwrap(), flags);
        assert_eq!(p.memory.read_int(DATA + 0xa58, 4).unwrap(), change);
    }
}

#[test]
fn input_events_win64_native_activation_updates_full_width_owners_and_notifications() {
    let (mut p, hwnd) = window();
    for (message, wp, query, expected) in [
        (0x1c, 1, "GetForegroundWindow", hwnd),
        (6, 1, "GetActiveWindow", hwnd),
        (7, 0, "GetFocus", hwnd),
        (8, 0, "GetFocus", 0),
        (6, 0, "GetActiveWindow", 0),
        (0x1c, 0, "GetForegroundWindow", 0),
    ] {
        host(&mut p, hwnd, message, wp, 0);
        assert_eq!(call(&mut p, query, &[]), expected);
        assert_eq!(peek(&mut p, hwnd, message, true), 1);
        assert_eq!(p.memory.read_int(DATA + 0x900, 8).unwrap(), hwnd);
        assert_eq!(p.memory.read_int(DATA + 0x908, 4).unwrap(), message as u64);
        assert_eq!(p.memory.read_int(DATA + 0x910, 8).unwrap(), wp as u64);
    }
}

#[test]
fn input_events_win64_modifier_sides_and_all_buttons_commit_only_when_removed() {
    let (mut p, hwnd) = window();
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
        let before = call(&mut p, "GetKeyState", &[specific]);
        host(&mut p, hwnd, message, key, lp);
        assert_eq!(
            call(&mut p, "GetAsyncKeyState", &[generic]) & 0x8000 != 0,
            held
        );
        assert_eq!(peek(&mut p, hwnd, message, false), 1);
        assert_eq!(call(&mut p, "GetKeyState", &[specific]), before);
        assert_eq!(peek(&mut p, hwnd, message, true), 1);
        assert_eq!(p.memory.read_int(DATA + 0x900, 8).unwrap(), hwnd);
        assert_eq!(p.memory.read_int(DATA + 0x910, 8).unwrap(), key as u64);
        assert_eq!(
            p.memory.read_int(DATA + 0x918, 8).unwrap(),
            lp as i32 as i64 as u64
        );
        assert_eq!(call(&mut p, "GetKeyState", &[generic]) & 0x8000 != 0, held);
        assert_eq!(
            call(&mut p, "GetKeyState", &[specific]) & 0x8000 != 0,
            matches!(message, 0x100 | 0x104 | 0x201 | 0x204 | 0x207 | 0x20b)
        );
    }
    host(&mut p, hwnd, 0x100, 65, 0x1e0001);
    assert_eq!(call(&mut p, "GetAsyncKeyState", &[65]), 0xffff8001);
    host(&mut p, hwnd, 0x100, 65, 0x401e0001);
    assert_eq!(call(&mut p, "GetAsyncKeyState", &[65]), 0xffff8000);
}

#[test]
fn input_events_win64_wheels_snapshot_guest_points_and_system_text_preserves_surrogates() {
    let (mut p, hwnd) = window();
    let local = 0x0019fffe;
    let screen = 1 | (29 << 16);
    for (message, wp, expected) in [
        (0x200, 12, local),
        (0x20a, 0xff10000c, screen),
        (0x20e, 0x78000c, screen),
    ] {
        host(&mut p, hwnd, message, wp, local);
        host(&mut p, hwnd, 0x200, 0, 0);
        assert_eq!(peek(&mut p, hwnd, message, true), 1);
        assert_eq!(p.memory.read_int(DATA + 0x910, 8).unwrap(), wp as u64);
        assert_eq!(p.memory.read_int(DATA + 0x918, 8).unwrap(), expected as u64);
        assert_eq!(p.memory.read_int(DATA + 0x924, 4).unwrap(), 1);
        assert_eq!(p.memory.read_int(DATA + 0x928, 4).unwrap(), 29);
        assert_eq!(call(&mut p, "GetMessagePos", &[]), screen as u64);
        while peek(&mut p, hwnd, 0x200, true) != 0 {}
    }
    let lp = 0x201e0001;
    host(&mut p, hwnd, 0x104, 65, lp);
    for c in [0x3042, 0xd83d, 0xde42] {
        host(&mut p, hwnd, 0x106, c, lp);
    }
    assert_eq!(peek(&mut p, hwnd, 0x106, true), 0);
    assert_eq!(peek(&mut p, hwnd, 0x104, true), 1);
    assert_eq!(call(&mut p, "TranslateMessage", &[DATA + 0x900]), 1);
    for c in [0x3042, 0xd83d, 0xde42] {
        assert_eq!(peek(&mut p, hwnd, 0x106, true), 1);
        assert_eq!(p.memory.read_int(DATA + 0x910, 8).unwrap(), c);
        assert_eq!(p.memory.read_int(DATA + 0x918, 8).unwrap(), lp as u64);
    }
    assert_eq!(peek(&mut p, hwnd, 0x106, true), 0);
}

fn register_raw(p: &mut Process64, target: u64, flags: u32) {
    for (n, usage) in [2, 6].into_iter().enumerate() {
        let at = DATA + 0x300 + n as u64 * 16;
        p.memory.write_int(at, 2, 1).unwrap();
        p.memory.write_int(at + 2, 2, usage).unwrap();
        p.memory.write_int(at + 4, 4, flags as u64).unwrap();
        p.memory.write_int(at + 8, 8, target).unwrap();
    }
    assert_eq!(
        call(p, "RegisterRawInputDevices", &[DATA + 0x300, 2, 16]),
        1
    );
}
fn raw_bytes(p: &mut Process64, handle: u64) -> Vec<u8> {
    let size = DATA + 0x400;
    assert_eq!(
        call(p, "GetRawInputData", &[handle, 0x10000003, 0, size, 24]),
        0
    );
    let len = p.memory.read_int(size, 4).unwrap();
    assert_eq!(
        call(
            p,
            "GetRawInputData",
            &[handle, 0x10000003, DATA + 0x500, size, 24]
        ),
        len
    );
    let mut bytes = vec![0; len as usize];
    p.memory.read(DATA + 0x500, &mut bytes).unwrap();
    bytes
}
fn field(bytes: &[u8], at: usize, width: usize) -> u64 {
    let mut value = [0; 8];
    value[..width].copy_from_slice(&bytes[at..at + width]);
    u64::from_le_bytes(value)
}

#[test]
fn raw_input_win64_routes_real_host_transitions_and_keeps_packets_until_default_cleanup() {
    let (mut p, hwnd) = window();
    register_raw(&mut p, hwnd, 0);
    let size = DATA + 0x400;
    assert_eq!(
        call(&mut p, "GetRegisteredRawInputDevices", &[0, size, 16]),
        0
    );
    assert_eq!(p.memory.read_int(size, 4).unwrap(), 2);
    assert_eq!(
        call(
            &mut p,
            "GetRegisteredRawInputDevices",
            &[DATA + 0x600, size, 16]
        ),
        2
    );
    assert_eq!(p.memory.read_int(DATA + 0x608, 8).unwrap(), hwnd);
    for (message, key, lp, kind, flags, detail) in [
        (0x200, 0, 100 | (200 << 16), 0, 0, 0),
        (0x200, 0, 113 | (191 << 16), 0, 0, 0),
        (0x201, 1, 113 | (191 << 16), 0, 1, 0),
        (0x202, 0, 113 | (191 << 16), 0, 2, 0),
        (
            0x20a,
            (-120i16 as u16 as u32) << 16,
            113 | (191 << 16),
            0,
            0x400,
            0xff88,
        ),
        (0x20b, 0x20040, 113 | (191 << 16), 0, 0x100, 0),
        (0x104, 18, 0x01380001, 1, 2, 0x38),
        (0x105, 18, 0xc1380001, 1, 3, 0x38),
    ] {
        host(&mut p, hwnd, message, key, lp);
        assert_eq!(peek(&mut p, hwnd, 0xff, true), 1);
        let handle = p.memory.read_int(DATA + 0x918, 8).unwrap();
        let bytes = raw_bytes(&mut p, handle);
        assert_eq!(field(&bytes, 0, 4), kind);
        assert_eq!(field(&bytes, 4, 4), bytes.len() as u64);
        assert_ne!(field(&bytes, 8, 8), 0);
        assert_eq!(field(&bytes, 16, 8), 0);
        if kind == 0 {
            assert_eq!(bytes.len(), 48);
            assert_eq!(field(&bytes, 24, 2), 3);
            assert_eq!(field(&bytes, 28, 2), flags);
            assert_eq!(field(&bytes, 30, 2), detail);
            if message == 0x200 && lp as u16 == 113 {
                assert_eq!(field(&bytes, 36, 4), 7424);
                assert_eq!(field(&bytes, 40, 4), 16640);
                assert_eq!(field(&bytes, 36, 4) * 1024 / 65535, 116);
                assert_eq!(field(&bytes, 40, 4) * 768 / 65535, 195);
            }
        } else {
            assert_eq!(bytes.len(), 40);
            assert_eq!(field(&bytes, 24, 2), detail);
            assert_eq!(field(&bytes, 26, 2), flags);
            assert_eq!(field(&bytes, 30, 2), key as u64);
            assert_eq!(field(&bytes, 32, 4), message as u64);
        }
        assert_eq!(raw_bytes(&mut p, handle), bytes);
        assert_eq!(peek(&mut p, hwnd, message, true), 1);
        assert_eq!(call(&mut p, "DefWindowProcW", &[hwnd, 0xff, 0, handle]), 0);
        assert_eq!(
            call(
                &mut p,
                "GetRawInputData",
                &[handle, 0x10000003, 0, size, 24]
            ),
            u32::MAX as u64
        );
    }
}

#[test]
fn raw_input_win64_buffer_copy_is_atomic_and_excludes_removed_messages() {
    let (mut p, hwnd) = window();
    register_raw(&mut p, 0, 0x30);
    let size = DATA + 0x400;
    host(&mut p, hwnd, 0x201, 1, 20 | (30 << 16));
    assert_eq!(peek(&mut p, hwnd, 0x201, true), 0);
    assert_eq!(peek(&mut p, hwnd, 0xff, false), 1);
    let first = p.memory.read_int(DATA + 0x918, 8).unwrap();
    p.memory.write_int(size, 4, 47).unwrap();
    assert_eq!(
        call(&mut p, "GetRawInputBuffer", &[DATA + 0x500, size, 24]),
        u32::MAX as u64
    );
    assert_eq!(p.memory.read_int(size, 4).unwrap(), 48);
    assert_eq!(peek(&mut p, hwnd, 0xff, true), 1);
    host(&mut p, hwnd, 0x202, 0, 20 | (30 << 16));
    host(&mut p, hwnd, 0x100, 65, 0x001e0001);
    host(&mut p, hwnd, 0x102, 65, 0x001e0001);
    assert_eq!(peek(&mut p, hwnd, 0x102, true), 0);
    p.memory.write_int(size, 4, 96).unwrap();
    p.cpu.regs[4] = STACK + 0x1f88;
    p.memory.write_int(STACK + 0x1f88, 8, BASE).unwrap();
    p.cpu.regs[1] = DATA + 4096 - 16;
    p.cpu.regs[2] = size;
    p.cpu.regs[8] = 24;
    assert!(p
        .runtime
        .dispatch("user32.dll", "GetRawInputBuffer", &mut p.cpu, &mut p.memory)
        .is_err());
    assert_eq!(p.memory.read_int(size, 4).unwrap(), 96);
    assert_eq!(peek(&mut p, hwnd, 0xff, false), 1);
    assert_eq!(
        call(&mut p, "GetRawInputBuffer", &[DATA + 0x500, size, 24]),
        2
    );
    assert_eq!(p.memory.read_int(size, 4).unwrap(), 88);
    assert_eq!(p.memory.read_int(DATA + 0x500, 4).unwrap(), 0);
    assert_eq!(p.memory.read_int(DATA + 0x500 + 48, 4).unwrap(), 1);
    assert_eq!(peek(&mut p, hwnd, 0xff, true), 0);
    assert_eq!(call(&mut p, "GetKeyState", &[1]), 0);
    assert_eq!(call(&mut p, "GetKeyState", &[65]), 0xffff8000);
    assert_eq!(raw_bytes(&mut p, first).len(), 48);
    call(&mut p, "DefWindowProcW", &[hwnd, 0xff, 0, first]);
    assert_eq!(call(&mut p, "GetRawInputBuffer", &[0, size, 24]), 0);
    assert_eq!(p.memory.read_int(size, 4).unwrap(), 0);
}
