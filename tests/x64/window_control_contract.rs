//! WNDPROC capability/frame validation precedes window lookup in the old path.
use super::*;

#[test]
fn window_control_contract_win64_keeps_absence_and_callback_frame_validation_order() {
    use wire::x64::memory::{PAGE, READ, WRITE};
    let (mut p, _) = super::window_queries_contract::window();
    for name in ["DefDlgProcA", "DefDlgProcW", "EnableWindow", "SetParent"] {
        assert!(!wire::x64::runtime::import_supported("user32.dll", name));
    }
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll"] {
        for name in [
            "CreateWindowExA",
            "CreateWindowExW",
            "DestroyWindow",
            "DefWindowProcA",
            "DefWindowProcW",
            "CallWindowProcA",
            "CallWindowProcW",
            "ShowWindow",
            "UpdateWindow",
            "MoveWindow",
            "SetWindowPos",
            "SendMessageA",
            "SendMessageW",
        ] {
            assert!(wire::x64::runtime::import_supported(dll, name));
        }
    }
    let page = 0x450000;
    p.memory.map(page, PAGE, READ | WRITE).unwrap();
    // The API return slot is valid; the callback frame immediately below it
    // crosses into an unmapped page. Even invalid HWND=1 must fail here first.
    p.memory.write_int(page + 8, 8, BASE).unwrap();
    startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "SetLastError",
        &[77],
    );
    p.cpu.regs[4] = page + 8;
    p.cpu.regs[1] = 1;
    p.cpu.regs[2] = 5;
    let before = p.cpu.clone();
    let calls = p.runtime.api_calls;
    let error = p
        .runtime
        .dispatch("user32.dll", "ShowWindow", &mut p.cpu, &mut p.memory)
        .unwrap_err();
    assert!(error.contains("unmapped AMD64 address"), "{error}");
    assert_eq!(p.cpu, before);
    assert_eq!(p.runtime.api_calls, calls);
    assert_eq!(
        p.memory
            .read_int(wire::x64::runtime::TEB + 0x68, 4)
            .unwrap(),
        77
    );
}
