//! Independent Win64 query ABI and a real callback-created logical window.
use super::*;
use wire::x64::process::{Outcome, Process64};

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

pub(super) fn window() -> (Process64, u64) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "wire-query64-{}-{}.exe",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, fixture()).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    std::fs::remove_file(path).unwrap();
    let entry = p.cpu.clone();
    let record = BASE + 0x1200;
    let name = BASE + 0x1300;
    p.memory.write(name, b"QueryWindow\0").unwrap();
    let proc = p.runtime.register("user32.dll", "DefWindowProcW");
    for (offset, width, value) in [
        (0, 4, 80),
        (8, 8, proc),
        (20, 4, 16),
        (24, 8, BASE),
        (64, 8, name),
    ] {
        p.memory.write_int(record + offset, width, value).unwrap();
    }
    assert_ne!(call(&mut p, "RegisterClassExA", &[record]), 0);
    let create = p.runtime.register("user32.dll", "CreateWindowExA");
    let mut code = vec![0x48, 0x83, 0xec, 0x68];
    for (i, arg) in [0, name, name, 0x10000000, 10, 20, 640, 480, 0, 0, BASE, 0]
        .into_iter()
        .enumerate()
    {
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
    code.extend(create.to_le_bytes());
    code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x68, 0xc3]);
    p.memory.write(BASE + 0x1000, &code).unwrap();
    p.cpu = entry;
    let Outcome::Returned(hwnd) = p.run(2000).unwrap() else {
        panic!("window creation did not return")
    };
    assert_ne!(hwnd, 0);
    (p, hwnd)
}

#[test]
fn window_queries_contract_win64_keeps_aliases_absent_and_invalid_handle_errors() {
    let (mut mem, mut cpu, mut rt) = startup();
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll"] {
        let text = STACK + 0x200;
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "IsWindow",
            "IsWindowVisible",
            "IsWindowUnicode",
            "GetParent",
            "GetWindowThreadProcessId",
            "GetClientRect",
            "GetWindowRect",
            "GetWindowLongA",
            "GetWindowLongW",
            "SetWindowLongA",
            "SetWindowLongW",
            "GetWindowLongPtrA",
            "GetWindowLongPtrW",
            "SetWindowLongPtrA",
            "SetWindowLongPtrW",
            "GetWindowPlacement",
            "IsIconic",
            "ClientToScreen",
            "ScreenToClient",
        ] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text]
                ) != 0,
                dll == "user32.dll",
                "{dll}!{name}"
            );
        }
        for name in [
            "FindWindowA",
            "FindWindowW",
            "GetDesktopWindow",
            "IsZoomed",
            "IsWindowEnabled",
            "MapWindowPoints",
        ] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text]
                ),
                0
            );
        }
    }
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[77]);
    assert_eq!(
        startup_dll_call(&mut mem, &mut cpu, &mut rt, "user32.dll", "IsWindow", &[1]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        77
    );
    for name in [
        "IsWindowVisible",
        "IsWindowUnicode",
        "IsIconic",
        "GetParent",
        "GetWindowRect",
        "GetWindowPlacement",
    ] {
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut rt, "user32.dll", name, &[1, 0]),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            1400
        );
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "SetWindowLongPtrW",
            &[1, (-4i32) as u32 as u64, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    cpu.regs[1] = 1;
    cpu.regs[2] = (-4i32) as u32 as u64;
    cpu.regs[8] = 0x30000;
    assert!(rt
        .dispatch("user32.dll", "SetWindowLongPtrW", &mut cpu, &mut mem)
        .is_err());
}

#[test]
fn window_queries_contract_win64_keeps_field_widths_and_checked_outputs() {
    let (mut p, hwnd) = window();
    let value = 0xfedcba9876543210;
    assert_eq!(call(&mut p, "IsWindowVisible", &[hwnd]), 1);
    assert_eq!(call(&mut p, "IsWindowUnicode", &[hwnd]), 0);
    assert_eq!(
        call(
            &mut p,
            "SetWindowLongPtrW",
            &[hwnd, (-21i32) as u32 as u64, value]
        ),
        0
    );
    assert_eq!(
        call(&mut p, "GetWindowLongPtrA", &[hwnd, (-21i32) as u32 as u64]),
        value
    );
    assert_eq!(
        call(&mut p, "GetWindowLongA", &[hwnd, (-21i32) as u32 as u64]),
        0
    );
    assert_eq!(call(&mut p, "SetWindowLongPtrW", &[hwnd, 0, value]), 0);
    assert_eq!(call(&mut p, "GetWindowLongW", &[hwnd, 4]), value >> 32);
    assert_eq!(
        call(&mut p, "SetWindowLongA", &[hwnd, 4, u64::MAX]),
        value >> 32
    );
    assert_eq!(
        call(&mut p, "GetWindowLongPtrA", &[hwnd, 0]),
        0xffffffff76543210
    );
    assert_eq!(call(&mut p, "GetWindowLongPtrW", &[hwnd, 9]), 0);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        1413
    );
    let out = BASE + 0x1380;
    assert_ne!(call(&mut p, "GetWindowThreadProcessId", &[hwnd, out]), 0);
    assert_eq!(p.memory.read_int(out, 4).unwrap(), 1);
    p.memory.write_int(out, 4, 44).unwrap();
    assert_eq!(call(&mut p, "GetWindowPlacement", &[hwnd, out]), 1);
    assert_eq!(p.memory.read_int(out + 8, 4).unwrap(), 1);
    assert_eq!(p.memory.read_int(out + 28, 4).unwrap(), 10);
    // Process64 maps a larger stack than the small startup() harness.
    let page = 0x6c0000000;
    p.memory.map(page, 4096, READ | WRITE).unwrap();
    let edge = page + 4096 - 8;
    p.memory.write(edge, &[0xaa; 8]).unwrap();
    p.cpu.regs[1] = hwnd;
    p.cpu.regs[2] = edge;
    assert!(p
        .runtime
        .dispatch("user32.dll", "GetWindowRect", &mut p.cpu, &mut p.memory)
        .is_err());
    assert_eq!(p.memory.read_int(edge, 8).unwrap(), 0xaaaaaaaaaaaaaaaa);
    p.cpu.regs[1] = hwnd;
    p.cpu.regs[2] = (-8i32) as u32 as u64;
    p.cpu.regs[8] = 0;
    assert!(p
        .runtime
        .dispatch("user32.dll", "SetWindowLongPtrW", &mut p.cpu, &mut p.memory)
        .unwrap_err()
        .contains("reparenting"));
}
