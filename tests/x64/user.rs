//! Existing behavioral tests grouped by user; expectations remain independent of API catalogs.

use super::*;

fn keyboard_call(
    m: &mut Memory,
    cpu: &mut Cpu64,
    r: &mut wire::x64::runtime::Runtime64,
    name: &str,
    args: &[u64],
) -> u64 {
    startup_dll_call(m, cpu, r, "user32.dll", name, args)
}

#[test]
fn win64_keyboard_layout_queries_keep_hkl_width_thread_ids_and_dll_ownership() {
    let (mut m, mut cpu, mut r) = startup();
    let out = STACK + 0x100;
    let hkl = keyboard_call(&mut m, &mut cpu, &mut r, "GetKeyboardLayout", &[0]);
    assert_eq!(hkl, 0x04090409);
    assert_eq!(
        keyboard_call(
            &mut m,
            &mut cpu,
            &mut r,
            "GetKeyboardLayout",
            &[0xaaaa00000001]
        ),
        hkl
    );
    assert_eq!(
        keyboard_call(&mut m, &mut cpu, &mut r, "GetKeyboardLayout", &[999]),
        0
    );
    assert_eq!(
        keyboard_call(&mut m, &mut cpu, &mut r, "GetKeyboardLayoutList", &[0, 1]),
        1
    );
    m.write(out, &[0xaa; 24]).unwrap();
    assert_eq!(
        keyboard_call(&mut m, &mut cpu, &mut r, "GetKeyboardLayoutList", &[8, out]),
        1
    );
    assert_eq!(m.read_int(out, 8).unwrap(), hkl);
    assert_eq!(m.read_int(out + 8, 8).unwrap(), 0xaaaaaaaaaaaaaaaa);
    assert_eq!(
        keyboard_call(
            &mut m,
            &mut cpu,
            &mut r,
            "GetKeyboardLayoutList",
            &[u32::MAX as u64, out]
        ),
        0
    );
    for (name, expected) in [
        ("GetKeyboardLayoutNameA", b"00000409\0".to_vec()),
        (
            "GetKeyboardLayoutNameW",
            "00000409\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect(),
        ),
    ] {
        assert_eq!(keyboard_call(&mut m, &mut cpu, &mut r, name, &[out]), 1);
        let mut data = vec![0; expected.len()];
        m.read(out, &mut data).unwrap();
        assert_eq!(data, expected);
    }
    for (code, expected) in [(0, 4), (1, 0), (2, 12), (3, 0)] {
        assert_eq!(
            keyboard_call(&mut m, &mut cpu, &mut r, "GetKeyboardType", &[code]),
            expected
        );
    }
    m.write(out, b"user32.dll\0").unwrap();
    r.register_provider("user32.dll");
    let module = startup_call(&mut m, &mut cpu, &mut r, "LoadLibraryA", &[out]);
    for name in [
        "GetKeyboardLayout",
        "GetKeyboardLayoutList",
        "GetKeyboardLayoutNameW",
        "MapVirtualKeyExA",
        "ToUnicode",
        "ToUnicodeEx",
        "GetKeyNameTextW",
    ] {
        assert!(wire::x64::runtime::import_supported("user32.dll", name));
        for dll in ["kernel32.dll", "kernelbase.dll", "imm32.dll"] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
        }
        m.write(out, format!("{name}\0").as_bytes()).unwrap();
        let gateway = startup_call(&mut m, &mut cpu, &mut r, "GetProcAddress", &[module, out]);
        assert_eq!(r.gateways[&gateway], format!("user32.dll!{name}"));
    }
    // Explicit layouts remain full-width handles, not truncated DWORDs.
    assert_eq!(
        keyboard_call(
            &mut m,
            &mut cpu,
            &mut r,
            "MapVirtualKeyExW",
            &[65, 0, hkl | (1 << 32)]
        ),
        0
    );
}

#[test]
fn win64_keyboard_scan_maps_and_unicode_use_explicit_state_without_consuming_it() {
    let (mut m, mut cpu, mut r) = startup();
    let state_ptr = STACK + 0x200;
    let out = STACK + 0x400;
    for name in [
        "MapVirtualKeyA",
        "MapVirtualKeyW",
        "MapVirtualKeyExA",
        "MapVirtualKeyExW",
    ] {
        for (code, kind, expected) in [
            (65, 0, 0x1e),
            (0x1e, 1, 65),
            (65, 2, 65),
            (0xba, 2, 59),
            (0xa3, 4, 0xe01d),
            (0xe01d, 3, 0xa3),
            (0xe01d, 1, 0x11),
            (0x36, 3, 0xa1),
            (0x36, 1, 0x10),
            (0x67, 0, 0x47),
            (0x47, 1, 0x24),
            (0xdead, 1, 0),
            (65, 99, 0),
        ] {
            let mut args = vec![code | (0xabcd << 32), kind | (0xabcd << 32)];
            if name.contains("Ex") {
                args.push(0x04090409);
            }
            assert_eq!(
                keyboard_call(&mut m, &mut cpu, &mut r, name, &args),
                expected
            );
        }
    }
    for (vk, shift, caps, ctrl, alt, expected) in [
        (65, false, false, false, false, Some(97)),
        (65, true, false, false, false, Some(65)),
        (65, false, true, false, false, Some(65)),
        (65, true, true, false, false, Some(97)),
        (65, false, false, true, false, Some(1)),
        (65, false, false, true, true, None),
        (0x32, true, false, true, false, Some(0)),
        (0xdb, false, false, true, false, Some(27)),
        (0xba, true, false, false, false, Some(58)),
        (0x67, false, false, false, false, Some(55)),
        (0x67, true, false, false, false, None),
        (0x70, false, false, false, false, None),
    ] {
        let mut state = [0; 256];
        state[0x10] = if shift { 0x80 } else { 0 };
        state[0x14] = caps as u8;
        state[0x11] = if ctrl { 0x80 } else { 0 };
        state[0x12] = if alt { 0x80 } else { 0 };
        m.write(state_ptr, &state).unwrap();
        for name in ["ToUnicode", "ToUnicodeEx"] {
            let mut args = vec![vk, 0, state_ptr, out, 2, 4];
            if name.ends_with("Ex") {
                args.push(0x04090409);
            }
            for _ in 0..2 {
                m.write_int(out, 4, u32::MAX as u64).unwrap();
                assert_eq!(
                    keyboard_call(&mut m, &mut cpu, &mut r, name, &args),
                    expected.is_some() as u64
                );
                assert_eq!(m.read_int(out, 4).unwrap(), expected.unwrap_or(0));
            }
        }
        let mut after = [0; 256];
        m.read(state_ptr, &mut after).unwrap();
        assert_eq!(after, state);
    }
    m.write(state_ptr, &[0; 256]).unwrap();
    assert_eq!(
        keyboard_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ToUnicode",
            &[65, 0x801e, state_ptr, out, 2, 0]
        ),
        0
    );
    assert_eq!(
        keyboard_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ToUnicode",
            &[65, 0x801e, state_ptr, out, 1, 2]
        ),
        1
    );
    assert_eq!(m.read_int(out, 2).unwrap(), 97);
    // Zero/negative lengths do not dereference input/output pointers.
    for size in [0, u32::MAX as u64] {
        assert_eq!(
            keyboard_call(
                &mut m,
                &mut cpu,
                &mut r,
                "ToUnicode",
                &[65, 0, 1, 1, size, 0]
            ),
            0
        );
    }
}

#[test]
fn win64_keyboard_key_names_truncate_terminate_and_fold_modifier_sides() {
    let (mut m, mut cpu, mut r) = startup();
    let out = STACK + 0x100;
    for name in ["GetKeyNameTextA", "GetKeyNameTextW"] {
        for (lparam, size, expected) in [
            (0x01e0000, 32, "A"),
            (0x11d0000, 32, "Right Ctrl"),
            (0x31d0000, 32, "Ctrl"),
            (0x1470000, 32, "Home"),
            (0x0470000, 32, "Num 7"),
            (0x11d0000, 4, "Rig"),
            (0x11d0000, 1, ""),
            (0x07f0000, 32, ""),
        ] {
            m.write(out, &[0xcc; 64]).unwrap();
            assert_eq!(
                keyboard_call(&mut m, &mut cpu, &mut r, name, &[lparam, out, size]),
                expected.len() as u64
            );
            let expected: Vec<u8> = if name.ends_with('W') {
                expected
                    .encode_utf16()
                    .chain([0])
                    .flat_map(u16::to_le_bytes)
                    .collect()
            } else {
                expected.bytes().chain([0]).collect()
            };
            let mut data = vec![0; expected.len()];
            m.read(out, &mut data).unwrap();
            assert_eq!(data, expected);
            assert_eq!(m.read_int(out + data.len() as u64, 1).unwrap(), 0xcc);
        }
    }
}

#[test]
fn win64_keyboard_cross_page_faults_preserve_outputs_cpu_and_completed_calls() {
    let (mut m, mut cpu, mut r) = startup();
    const DATA: u64 = 0x700000000;
    m.map(DATA, 8192, READ | WRITE).unwrap();
    m.write(STACK + 0x200, &[0; 256]).unwrap();
    for (name, args, read_fault) in [
        ("GetKeyboardLayoutNameW", vec![DATA + 4090], false),
        ("GetKeyboardLayoutList", vec![1, DATA + 4092], false),
        ("GetKeyNameTextW", vec![0x11d0000, DATA + 4092, 32], false),
        (
            "ToUnicode",
            vec![65, 0, STACK + 0x200, DATA + 4094, 2, 4],
            false,
        ),
        ("ToUnicode", vec![65, 0, DATA + 4090, DATA, 2, 4], true),
    ] {
        m.protect(DATA + 4096, 4096, READ | WRITE).unwrap();
        m.write(DATA, &vec![0xaa; 8192]).unwrap();
        cpu.regs[4] = STACK + 0x1f88;
        m.write_int(cpu.regs[4], 8, BASE).unwrap();
        for (i, value) in args.into_iter().enumerate() {
            if i < 4 {
                cpu.regs[[1, 2, 8, 9][i]] = value;
            } else {
                m.write_int(cpu.regs[4] + 40 + (i as u64 - 4) * 8, 8, value)
                    .unwrap();
            }
        }
        m.protect(DATA + 4096, 4096, if read_fault { 0 } else { READ })
            .unwrap();
        let before = cpu.clone();
        let calls = r.api_calls;
        assert!(r.dispatch("user32.dll", name, &mut cpu, &mut m).is_err());
        assert_eq!(cpu.regs, before.regs);
        assert_eq!(cpu.rip, before.rip);
        assert_eq!(r.api_calls, calls);
        let mut data = [0; 4096];
        m.read(DATA, &mut data).unwrap();
        assert_eq!(data, [0xaa; 4096]);
    }
}

#[test]
fn amd64_window_position_executes_mutable_win64_layout_and_nested_default_messages_across_budgets()
{
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x6b0000000;
    const DATA: u64 = CODE + 4096;
    fn mov(code: &mut Vec<u8>, r: u8, value: u64) {
        code.extend([0x48 | (r >> 3), 0xb8 | (r & 7)]);
        code.extend(value.to_le_bytes());
    }
    fn call(code: &mut Vec<u8>, gateway: u64, args: &[u64], window: bool) {
        for (n, &arg) in args.iter().enumerate() {
            if window && n == 0 {
                code.extend([0x48, 0x89, 0xd9]);
            } else if n < 4 {
                mov(code, [1, 2, 8, 9][n], arg);
            } else {
                mov(code, 0, arg);
                code.extend([0x48, 0x89, 0x44, 0x24, 32 + (n as u8 - 4) * 8]);
            }
        }
        mov(code, 0, gateway);
        code.extend([0xff, 0xd0]);
    }
    let path = std::env::temp_dir().join(format!("wire-windowpos64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    let original = p.cpu.clone();
    p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
    p.memory
        .write(
            DATA,
            &"PositionProbe\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let class = DATA + 0x100;
    p.memory.write_int(class, 4, 80).unwrap();
    p.memory.write_int(class + 8, 8, CODE).unwrap();
    p.memory.write_int(class + 24, 8, BASE).unwrap();
    p.memory.write_int(class + 64, 8, DATA).unwrap();
    assert_ne!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "RegisterClassExW",
            &[class]
        ),
        0
    );
    let create = p.runtime.register("user32.dll", "CreateWindowExW");
    let position = p.runtime.register("user32.dll", "SetWindowPos");
    let default = p.runtime.register("user32.dll", "DefWindowProcW");
    let mut callback = Vec::new();
    mov(&mut callback, 0, DATA + 0x400);
    callback.extend([
        0x48, 0xff, 0x00, 0x4c, 0x8b, 0x10, 0x42, 0x89, 0x54, 0xd0, 0,
    ]);
    // Record LPARAM for every message. The first positioning buffer is full-width.
    mov(&mut callback, 0, DATA + 0x500);
    callback.extend([0x4e, 0x89, 0x0c, 0xd0]);
    // Applications can alter position/size in WM_WINDOWPOSCHANGING.
    callback.extend([0x83, 0xfa, 0x46, 0x75, 16, 0x41, 0xc7, 0x41, 16]);
    callback.extend(30u32.to_le_bytes());
    callback.extend([0x41, 0xc7, 0x41, 24]);
    callback.extend(800u32.to_le_bytes());
    mov(&mut callback, 0, default);
    callback.extend([0xff, 0xe0]); // Real guest tail call, including nested MOVE/SIZE.
    p.memory.write(CODE, &callback).unwrap();
    let mut main = vec![0x48, 0x83, 0xec, 0x68];
    call(
        &mut main,
        create,
        &[8, DATA, DATA, 0, 10, 20, 640, 480, 0, 0, BASE, 0],
        false,
    );
    main.extend([0x48, 0x89, 0xc3]); // RBX = HWND.
    call(
        &mut main,
        position,
        &[0, u64::MAX - 1, 20, 40, 700, 600, 0x140],
        true,
    );
    main.extend([0x48, 0x83, 0xc4, 0x68, 0xc3]);
    p.memory.write(BASE + 0x1000, &main).unwrap();
    p.cpu = original;
    loop {
        match p.run(p.cpu.work + 1).unwrap() {
            Outcome::Budget => assert!(p.cpu.work < 600),
            Outcome::Returned(value) => {
                assert_eq!(value, 1);
                break;
            }
            result => panic!("unexpected {result:?}"),
        }
    }
    let expected = [0x24, 0x81, 0x83, 1, 0x46, 0x83, 0x47, 3, 5];
    assert_eq!(
        p.memory.read_int(DATA + 0x400, 8).unwrap(),
        expected.len() as u64
    );
    for (n, message) in expected.into_iter().enumerate() {
        assert_eq!(
            p.memory.read_int(DATA + 0x408 + n as u64 * 8, 4).unwrap(),
            message
        );
    }
    assert_eq!(p.memory.read_int(DATA + 0x540, 8).unwrap(), 30 | (40 << 16));
    assert_eq!(
        p.memory.read_int(DATA + 0x548, 8).unwrap(),
        800 | (600 << 16)
    );
    let buffer = p.memory.read_int(DATA + 0x528, 8).unwrap();
    assert!(buffer > u32::MAX as u64);
    assert!(p.memory.read_int(buffer, 1).is_err());
    let window = p.runtime.window_info()[0].clone();
    assert!(window.visible);
    assert!(!window.topmost);
    assert_eq!((window.width, window.height), (800, 600));
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "GetWindowRect",
            &[window.handle, DATA + 0x200]
        ),
        1
    );
    for (n, expected) in [30, 40, 830, 640].into_iter().enumerate() {
        assert_eq!(
            p.memory.read_int(DATA + 0x200 + n as u64 * 4, 4).unwrap(),
            expected
        );
    }
    p.memory.write_int(DATA + 0x200, 4, 44).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "GetWindowPlacement",
            &[window.handle, DATA + 0x200]
        ),
        1
    );
    assert_eq!(p.memory.read_int(DATA + 0x208, 4).unwrap(), 1);
    assert_eq!(p.memory.read_int(DATA + 0x20c, 8).unwrap(), u64::MAX);
    for (n, expected) in [30, 40, 830, 640].into_iter().enumerate() {
        assert_eq!(
            p.memory.read_int(DATA + 0x21c + n as u64 * 4, 4).unwrap(),
            expected
        );
    }
    p.memory.write_int(DATA + 0x200, 4, 48).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "GetWindowPlacement",
            &[window.handle, DATA + 0x200]
        ),
        0
    );
    assert_eq!(p.memory.read_int(DATA + 0x200, 4).unwrap(), 48);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "dwmapi.dll",
            "DwmGetWindowAttribute",
            &[window.handle, 9, DATA + 0x200, 16]
        ),
        0
    );
    assert_eq!(p.memory.read_int(DATA + 0x200, 4).unwrap(), 30);
    assert_eq!(p.memory.read_int(DATA + 0x20c, 4).unwrap(), 640);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "dwmapi.dll",
            "DwmGetWindowAttribute",
            &[window.handle, 9, DATA + 0x200, 12]
        ),
        0x80070057
    );
    assert_eq!(p.memory.read_int(DATA + 0x200, 4).unwrap(), 30);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "dwmapi.dll",
            "DwmIsCompositionEnabled",
            &[DATA + 0x200]
        ),
        0
    );
    assert_eq!(p.memory.read_int(DATA + 0x200, 4).unwrap(), 0);
    for (n, value) in [50u32, 60, 70, 80].into_iter().enumerate() {
        p.memory
            .write_int(DATA + 0x300 + n as u64 * 4, 4, value as u64)
            .unwrap();
    }
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "ClipCursor",
            &[DATA + 0x300]
        ),
        1
    );
    p.runtime
        .queue_window_event(wire::x64::runtime::WindowEvent {
            window: window.handle,
            message: 0x200,
            wparam: 0,
            lparam: 100 << 16,
        })
        .unwrap();
    let mouse = p
        .runtime
        .take_window_events()
        .into_iter()
        .find(|e| e.message == 0x200)
        .unwrap();
    assert_eq!(mouse.lparam, 20 | (39 << 16));
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "GetCursorPos",
            &[DATA + 0x200]
        ),
        1
    );
    assert_eq!(p.memory.read_int(DATA + 0x200, 8).unwrap(), 50 | (79 << 32));
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "ClipCursor",
            &[0]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "GetClipCursor",
            &[DATA + 0x200]
        ),
        1
    );
    assert_eq!(
        p.memory.read_int(DATA + 0x208, 8).unwrap(),
        1024 | (768 << 32)
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "SetWindowPos",
            &[window.handle, 0, 0, 0, 1, 1, 0x4000]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        50
    );
    assert_eq!(
        (
            p.runtime.window_info()[0].width,
            p.runtime.window_info()[0].height
        ),
        (800, 600)
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn amd64_window_creation_executes_messages_with_full_createstruct_and_rolls_back_callback_rejection(
) {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x6a0000000;
    const DATA: u64 = CODE + 4096;
    const PARAMETER: u64 = 0x123456789abcdef0;
    let path = std::env::temp_dir().join(format!("wire-window64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for (nccreate, create, expected) in [
        (1u32, 0u32, vec![0x24u64, 0x81, 0x83, 1]),
        (0, 0, vec![0x24, 0x81, 0x82]),
        (1, u32::MAX, vec![0x24, 0x81, 0x83, 1, 2, 0x82]),
    ] {
        let mut p = Process64::load(&path, false).unwrap();
        let entry = p.cpu.clone();
        p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
        p.memory
            .write(
                DATA,
                &"WireProbe\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        p.memory
            .write(
                DATA + 64,
                &"Actual guest window\0"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let class = DATA + 0x100;
        p.memory.write_int(class, 4, 80).unwrap();
        p.memory.write_int(class + 8, 8, CODE).unwrap();
        p.memory.write_int(class + 20, 4, 16).unwrap();
        p.memory.write_int(class + 24, 8, BASE).unwrap();
        p.memory.write_int(class + 64, 8, DATA).unwrap();
        let atom = startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "RegisterClassExW",
            &[class],
        );
        assert_ne!(atom, 0);
        p.cpu = entry;
        let gateway = p.runtime.register("user32.dll", "CreateWindowExW");
        let mut code = vec![0x48, 0x83, 0xec, 0x68];
        let args = [
            0,
            DATA,
            DATA + 64,
            0x00cf0000,
            10,
            20,
            640,
            480,
            0,
            0,
            BASE,
            PARAMETER,
        ];
        for (i, arg) in args.into_iter().enumerate() {
            if i < 4 {
                let (rex, op) = [(0x48, 0xb9), (0x48, 0xba), (0x49, 0xb8), (0x49, 0xb9)][i];
                code.extend([rex, op]);
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
        let mut callback = vec![0x48, 0xb8];
        callback.extend((DATA + 0x400).to_le_bytes());
        callback.extend([0x48, 0xff, 0x00, 0x48, 0x8b, 0x08, 0x89, 0x54, 0xc8, 0]); // record message
        let mut nc = vec![0x48, 0xb8];
        nc.extend((DATA + 0x500).to_le_bytes());
        nc.extend([
            0x4c, 0x89, 0x08, 0x49, 0x8b, 0x09, 0x48, 0x89, 0x48, 8, 0x49, 0x8b, 0x49, 8, 0x48,
            0x89, 0x48, 16,
        ]);
        nc.push(0xb8);
        nc.extend(nccreate.to_le_bytes());
        nc.push(0xc3);
        callback.extend([0x81, 0xfa, 0x81, 0, 0, 0, 0x75, nc.len() as u8]);
        callback.extend(nc);
        callback.extend([0x83, 0xfa, 1, 0x75, 6, 0xb8]);
        callback.extend(create.to_le_bytes());
        callback.push(0xc3);
        callback.extend([0xb8, 1, 0, 0, 0, 0xc3]);
        p.memory.write(CODE, &callback).unwrap();
        let result = loop {
            match p.run(p.cpu.work + 1).unwrap() {
                Outcome::Budget => assert!(p.cpu.work < 400),
                Outcome::Returned(value) => break value,
                result => panic!("unexpected {result:?}"),
            }
        };
        let success = nccreate != 0 && create != u32::MAX;
        assert_eq!(result != 0, success);
        assert_eq!(
            p.memory.read_int(DATA + 0x400, 8).unwrap(),
            expected.len() as u64
        );
        for (i, msg) in expected.into_iter().enumerate() {
            assert_eq!(
                p.memory.read_int(DATA + 0x408 + i as u64 * 8, 4).unwrap(),
                msg
            );
        }
        let temporary = p.memory.read_int(DATA + 0x500, 8).unwrap();
        assert!(p.memory.read_int(temporary, 1).is_err());
        assert_eq!(p.memory.read_int(DATA + 0x508, 8).unwrap(), PARAMETER);
        assert_eq!(p.memory.read_int(DATA + 0x510, 8).unwrap(), BASE);
        assert_eq!(p.cpu.regs[4], STACK + 0xfff70);
        if success {
            assert!(result > u32::MAX as u64);
            let info = p.runtime.window_info();
            assert_eq!(info.len(), 1);
            assert_eq!(info[0].handle, result);
            assert_eq!(info[0].title, "Actual guest window");
            assert!(!info[0].visible);
            assert_eq!((info[0].width, info[0].height), (640, 480));
            let event = wire::x64::runtime::WindowEvent {
                window: result,
                message: 0x401,
                wparam: PARAMETER,
                lparam: !PARAMETER,
            };
            p.runtime.queue_window_event(event).unwrap();
            let queued = p.runtime.take_window_events();
            assert_eq!(queued.len(), 1);
            assert_eq!(
                (queued[0].window, queued[0].wparam, queued[0].lparam),
                (result, PARAMETER, !PARAMETER)
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "IsWindow",
                    &[result]
                ),
                1
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "SetWindowLongPtrW",
                    &[result, (-21i32) as u32 as u64, PARAMETER]
                ),
                0
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "GetWindowLongPtrW",
                    &[result, (-21i32) as u32 as u64]
                ),
                PARAMETER
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "SetWindowLongPtrW",
                    &[result, 0, PARAMETER]
                ),
                0
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "GetWindowLongPtrW",
                    &[result, 0]
                ),
                PARAMETER
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "GetClientRect",
                    &[result, DATA + 0x200]
                ),
                1
            );
            assert_eq!(p.memory.read_int(DATA + 0x208, 4).unwrap(), 640);
            let dc = startup_dll_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "user32.dll",
                "GetDC",
                &[result],
            );
            assert!(dc > u32::MAX as u64);
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "ReleaseDC",
                    &[result + 16, dc]
                ),
                0
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "gdi32.dll",
                    "DeleteDC",
                    &[dc]
                ),
                0
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "gdi32.dll",
                    "GetDeviceCaps",
                    &[dc, 88]
                ),
                96
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "ReleaseDC",
                    &[result, dc]
                ),
                1
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "gdi32.dll",
                    "GetDeviceCaps",
                    &[dc, 88]
                ),
                0
            );
        } else {
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "IsWindow",
                    &[wire::x64::runtime::TEB + 0x630000]
                ),
                0
            );
            assert_eq!(
                startup_dll_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "user32.dll",
                    "UnregisterClassW",
                    &[DATA, BASE]
                ),
                1
            );
        }
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn amd64_icons_load_real_pe32plus_resources_keep_shared_lifetime_and_fail_missing_ids() {
    use wire::x64::process::Process64;
    let path = std::env::temp_dir().join(format!("wire-icon64-{}.exe", std::process::id()));
    let mut bytes = fixture();
    put32(&mut bytes, 0x98 + 112 + 16, 0x1100);
    put32(&mut bytes, 0x98 + 112 + 20, 0x200);
    // Separate GROUP_ICON and ICON branches, each with a real language/data entry.
    for (directory, entries) in [
        (0, vec![(3, 0x80000030), (14, 0x80000060)]),
        (0x30, vec![(5, 0x80000090)]),
        (0x60, vec![(103, 0x800000c0)]),
        (0x90, vec![(1033, 0xf0)]),
        (0xc0, vec![(1033, 0x100)]),
    ] {
        let base = 0x300 + directory;
        put16(&mut bytes, base + 14, entries.len() as u16);
        for (i, (name, value)) in entries.into_iter().enumerate() {
            put32(&mut bytes, base + 16 + i * 8, name);
            put32(&mut bytes, base + 20 + i * 8, value);
        }
    }
    put32(&mut bytes, 0x3f0, 0x1300);
    put32(&mut bytes, 0x3f4, 64);
    put32(&mut bytes, 0x400, 0x1340);
    put32(&mut bytes, 0x404, 20);
    put32(&mut bytes, 0x500, 40);
    put32(&mut bytes, 0x504, 2);
    put32(&mut bytes, 0x508, 4);
    put16(&mut bytes, 0x50c, 1);
    put16(&mut bytes, 0x50e, 32);
    bytes[0x542] = 1;
    bytes[0x544] = 1;
    bytes[0x546] = 2;
    bytes[0x547] = 2;
    put16(&mut bytes, 0x54a, 1);
    put16(&mut bytes, 0x54c, 32);
    put32(&mut bytes, 0x54e, 64);
    put16(&mut bytes, 0x552, 5);
    std::fs::write(&path, bytes).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    let icon = startup_dll_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "user32.dll",
        "LoadIconA",
        &[BASE, 103],
    );
    assert!(icon > u32::MAX as u64);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "LoadIconW",
            &[BASE, 103]
        ),
        icon
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "DestroyIcon",
            &[icon]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "LoadIconA",
            &[BASE, 103]
        ),
        icon
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "user32.dll",
            "LoadIconA",
            &[BASE, 104]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        1814
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn win64_display_queries_agree_on_one_monitor_and_check_ansi_wide_layouts_and_packed_points() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x400;
    let device = STACK + 0x200;
    for (suffix, size, flags, mode_size, size_offset, bits) in [
        ('A', 424u32, 164u64, 156u64, 36u64, 104u64),
        ('W', 840, 324, 220, 68, 168),
    ] {
        mem.write(output, &[0xcc; 848]).unwrap();
        mem.write_int(output, 4, size as u64).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "user32.dll",
                &format!("EnumDisplayDevices{suffix}"),
                &[0, 0, output, 0]
            ),
            1
        );
        assert_eq!(mem.read_int(output + flags, 4).unwrap(), 5);
        assert_eq!(
            mem.read_int(output + size as u64, 8).unwrap(),
            0xcccccccccccccccc
        );
        let mut before = vec![0; size as usize];
        mem.read(output, &mut before).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "user32.dll",
                &format!("EnumDisplayDevices{suffix}"),
                &[0, 1, output, 0]
            ),
            0
        );
        let mut after = vec![0; size as usize];
        mem.read(output, &mut after).unwrap();
        assert_eq!(after, before);
        let text: Vec<u8> = if suffix == 'W' {
            "\\\\.\\DISPLAY1\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            b"\\\\.\\DISPLAY1\0".to_vec()
        };
        mem.write(device, &text).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "user32.dll",
                &format!("EnumDisplayDevices{suffix}"),
                &[device, 0, output, 0]
            ),
            1
        );
        assert_eq!(mem.read_int(output + flags, 4).unwrap(), 3);
        for mode in [0, u32::MAX as u64, u32::MAX as u64 - 1] {
            mem.write(output, &[0xdd; 224]).unwrap();
            mem.write_int(output + size_offset, 2, mode_size).unwrap();
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    "user32.dll",
                    &format!("EnumDisplaySettings{suffix}"),
                    &[device, 0x1234000000000000 | mode, output]
                ),
                1
            );
            for (i, value) in [32, 1024, 768, 0, 60].into_iter().enumerate() {
                assert_eq!(
                    mem.read_int(output + bits + i as u64 * 4, 4).unwrap(),
                    value
                );
            }
            assert_eq!(mem.read_int(output + mode_size, 4).unwrap(), 0xdddddddd);
        }
    }
    let point = (100u64 << 32) | 100;
    let monitor = startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "user32.dll",
        "MonitorFromPoint",
        &[point, 0],
    );
    assert!(monitor > u32::MAX as u64);
    for point in [0x64ffffffff, 0xffffffff00000064, (768u64 << 32) | 1024] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "user32.dll",
                "MonitorFromPoint",
                &[point, 0]
            ),
            0
        );
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "user32.dll",
                "MonitorFromPoint",
                &[point, 2]
            ),
            monitor
        );
    }
    for (suffix, size) in [('A', 40), ('A', 72), ('W', 104)] {
        mem.write_int(output, 4, size).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "user32.dll",
                &format!("GetMonitorInfo{suffix}"),
                &[monitor, output]
            ),
            1
        );
        assert_eq!(mem.read_int(output + 12, 4).unwrap(), 1024);
        assert_eq!(mem.read_int(output + 16, 4).unwrap(), 768);
        assert_eq!(mem.read_int(output + 36, 4).unwrap(), 1);
    }
    // A valid cb field followed by an unmapped tail cannot partially write a device.
    let bad = STACK + 8192 - 8;
    mem.write_int(bad, 8, 0xcccccccc000001a8).unwrap();
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = 0;
    cpu.regs[2] = 0;
    cpu.regs[8] = bad;
    cpu.regs[9] = 0;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("user32.dll", "EnumDisplayDevicesA", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(mem.read_int(bad, 8).unwrap(), 0xcccccccc000001a8);
}

#[test]
fn amd64_monitor_enumeration_calls_real_guest_code_preserves_high_lparam_and_returns_callback_bool()
{
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x690000000;
    const DATA: u64 = CODE + 4096;
    let path = std::env::temp_dir().join(format!("wire-monitor64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for keep_going in [false, true] {
        let mut p = Process64::load(&path, false).unwrap();
        p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
        let gateway = p.runtime.register("user32.dll", "EnumDisplayMonitors");
        let parameter = 0x123456789abcdef0u64;
        let mut code = vec![0x48, 0x83, 0xec, 0x28, 0x31, 0xc9, 0x48, 0xba];
        code.extend(DATA.to_le_bytes());
        code.extend([0x49, 0xb8]);
        code.extend(CODE.to_le_bytes());
        code.extend([0x49, 0xb9]);
        code.extend(parameter.to_le_bytes());
        code.extend([0x48, 0xb8]);
        code.extend(gateway.to_le_bytes());
        code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(BASE + 0x1000, &code).unwrap();
        let mut callback = vec![0x48, 0xb8];
        callback.extend((DATA + 32).to_le_bytes());
        callback.extend([
            0x48, 0x89, 0x08, 0x48, 0x89, 0x50, 8, 0x4c, 0x89, 0x40, 16, 0x4c, 0x89, 0x48, 24,
        ]);
        for i in 0..4u8 {
            callback.extend([0x41, 0x8b, 0x48, 4 * i, 0x89, 0x48, 32 + 4 * i]);
        }
        callback.extend([0xb8, keep_going as u8, 0, 0, 0, 0xc3]);
        p.memory.write(CODE, &callback).unwrap();
        p.memory
            .write(
                DATA,
                &[10i32, 20, 100, 200]
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        loop {
            match p.run(p.cpu.work + 1).unwrap() {
                Outcome::Budget => assert!(p.cpu.work < 100),
                result => {
                    assert_eq!(result, Outcome::Returned(keep_going as u64));
                    break;
                }
            }
        }
        assert!(p.memory.read_int(DATA + 32, 8).unwrap() > u32::MAX as u64);
        assert_eq!(p.memory.read_int(DATA + 40, 8).unwrap(), 0);
        assert_eq!(p.memory.read_int(DATA + 56, 8).unwrap(), parameter);
        let block = p.memory.read_int(DATA + 48, 8).unwrap();
        assert!(p.memory.read_int(block, 1).is_err());
        for (i, value) in [10, 20, 100, 200].into_iter().enumerate() {
            assert_eq!(
                p.memory.read_int(DATA + 64 + i as u64 * 4, 4).unwrap(),
                value
            );
        }
        assert_eq!(p.cpu.regs[4], STACK + 0xfff70);
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn win64_ime_without_input_context_fails_before_touching_composition_buffers() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x400;
    let sentinel = [0xa5; 32];
    mem.write(output, &sentinel).unwrap();
    for context in [0, 0x1234567890] {
        for (name, args) in [
            ("ImmGetCompositionStringA", vec![context, 8, output, 32]),
            ("ImmGetCompositionStringW", vec![context, 8, output, 32]),
            ("ImmGetConversionStatus", vec![context, output, output + 4]),
            (
                "ImmSetCompositionStringA",
                vec![context, 9, 0xbad, 32, 0xbad, 32],
            ),
            (
                "ImmSetCompositionStringW",
                vec![context, 9, 0xbad, 32, 0xbad, 32],
            ),
            ("ImmSetOpenStatus", vec![context, 1]),
            ("ImmSetCompositionFontW", vec![context, 0xbad]),
            ("ImmSetCompositionWindow", vec![context, 0xbad]),
            ("ImmNotifyIME", vec![context, 0x15, 4, 0]),
        ] {
            assert!(wire::x64::runtime::import_supported("imm32.dll", name));
            assert_eq!(
                startup_dll_call(&mut mem, &mut cpu, &mut runtime, "imm32.dll", name, &args),
                0
            );
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
                6
            );
            let mut bytes = [0; 32];
            mem.read(output, &mut bytes).unwrap();
            assert_eq!(bytes, sentinel);
        }
    }
}
