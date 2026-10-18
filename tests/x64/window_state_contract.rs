//! Independent baseline for Win64 scalar titles and input ownership differences.
use super::*;
use wire::x64::process::Process64;

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

#[test]
fn window_state_contract_win64_keeps_provider_absence_and_null_rejection() {
    let (mut p, hwnd) = super::window_queries_contract::window();
    for name in [
        "SetActiveWindow",
        "GetLastActivePopup",
        "GetCapture",
        "SetCapture",
        "ReleaseCapture",
    ] {
        assert!(!wire::x64::runtime::import_supported("user32.dll", name));
    }
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll"] {
        for name in [
            "GetFocus",
            "GetActiveWindow",
            "GetForegroundWindow",
            "SetFocus",
            "SetForegroundWindow",
            "SetWindowTextA",
            "SetWindowTextW",
            "GetWindowTextA",
            "GetWindowTextW",
            "GetWindowTextLengthA",
            "GetWindowTextLengthW",
        ] {
            assert!(wire::x64::runtime::import_supported(dll, name));
        }
    }
    assert_eq!(call(&mut p, "SetFocus", &[hwnd]), 0);
    assert_eq!(call(&mut p, "SetFocus", &[0]), 0);
    assert_eq!(call(&mut p, "GetFocus", &[]), hwnd);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        1400
    );
    assert_eq!(call(&mut p, "SetForegroundWindow", &[hwnd]), 1);
    assert_eq!(call(&mut p, "SetForegroundWindow", &[0]), 0);
    assert_eq!(call(&mut p, "GetActiveWindow", &[]), hwnd);
    assert_eq!(call(&mut p, "GetForegroundWindow", &[]), hwnd);
}

#[test]
fn window_state_contract_win64_keeps_text_lengths_capacity_width_and_faults() {
    let (mut p, hwnd) = super::window_queries_contract::window();
    let text = BASE + 0x1380;
    let out = BASE + 0x1400;
    p.memory
        .write(
            text,
            &"日本A\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(call(&mut p, "SetWindowTextW", &[hwnd, text]), 1);
    assert_eq!(call(&mut p, "GetWindowTextLengthW", &[hwnd]), 3);
    assert_eq!(call(&mut p, "GetWindowTextLengthA", &[hwnd]), 5);
    assert_eq!(
        call(&mut p, "GetWindowTextW", &[hwnd, out, 0xffff000000000002]),
        1
    );
    assert_eq!(p.memory.read_int(out, 4).unwrap(), 0x65e5);
    assert_eq!(call(&mut p, "GetWindowTextA", &[hwnd, out, 2]), 1);
    assert_eq!(p.memory.read_int(out, 2).unwrap(), 0x0093);
    assert_eq!(call(&mut p, "GetWindowTextW", &[hwnd, 0x30000, 0]), 0);
    assert_eq!(call(&mut p, "SetWindowTextW", &[1, 0x30000]), 0);
    p.cpu.regs[1] = hwnd;
    p.cpu.regs[2] = 0x30000;
    assert!(p
        .runtime
        .dispatch("user32.dll", "SetWindowTextW", &mut p.cpu, &mut p.memory)
        .is_err());
    assert_eq!(call(&mut p, "GetWindowTextLengthW", &[hwnd]), 3);
    p.cpu.regs[1] = hwnd;
    p.cpu.regs[2] = 0;
    p.cpu.regs[8] = 1;
    assert!(p
        .runtime
        .dispatch("user32.dll", "GetWindowTextW", &mut p.cpu, &mut p.memory)
        .is_err());
    assert_eq!(call(&mut p, "SetWindowTextW", &[hwnd, 0]), 1);
    assert_eq!(call(&mut p, "GetWindowTextLengthW", &[hwnd]), 0);
}
