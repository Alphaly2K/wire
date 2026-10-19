//! Original update-region and erase-callback commit rules before migration.
use super::*;

#[test]
fn painting_contract_pe32_keeps_provider_and_invalid_window_before_pointer_checks() {
    let mut v = Vm::new();
    for (name, cleanup) in [
        ("InvalidateRect", 12),
        ("ValidateRect", 8),
        ("GetUpdateRect", 12),
        ("BeginPaint", 8),
        ("EndPaint", 8),
    ] {
        v.k("SetLastError", &[77]);
        assert_eq!(v.call("user32.dll", name, &[1, 0x30000, 1]), 0);
        assert_eq!(v.state.last_error, 1400, "{name}");
        assert_eq!(v.cpu.eip, RET);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4 + cleanup);
        assert!(v
            .log
            .recent
            .back()
            .unwrap()
            .ends_with(" ; window update/paint state"));
        for dll in ["kernel32.dll", "kernelbase.dll", "gdi32.dll"] {
            v.k("SetLastError", &[77]);
            assert_eq!(v.call(dll, name, &[1, 0x30000, 1]), 0);
            assert_eq!(v.state.last_error, 120, "{dll}!{name}");
            assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
            assert!(v.log.recent.back().unwrap().contains(
                "[UNIMPLEMENTED] ; unknown stdcall arity, assuming cdecl/0-byte cleanup"
            ));
        }
    }
}

#[test]
fn painting_contract_pe32_keeps_paintstruct_write_before_return_frame_read() {
    let mut v = Vm::new();
    let procedure = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, procedure);
    assert_eq!(v.call("user32.dll", "InvalidateRect", &[hwnd, 0, 0]), 1);
    let sp = STACK_BASE + STACK_SIZE - 256;
    v.k("SetLastError", &[77]);
    let dc = v.call("user32.dll", "BeginPaint", &[hwnd, sp]);
    assert_ne!(dc, 0);
    assert_eq!(v.cpu.eip, dc);
    assert_eq!(v.cpu.esp(), sp + 12);
    assert_eq!(v.mem.read_u32(sp).unwrap(), dc);
    assert_eq!(v.mem.read_u32(sp + 4).unwrap(), 0);
    assert_eq!(v.state.last_error, 77);
}

#[test]
fn painting_contract_pe32_keeps_update_output_alias_and_ignored_erase_argument() {
    let mut v = Vm::new();
    let procedure = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, procedure);
    assert_eq!(v.call("user32.dll", "InvalidateRect", &[hwnd, 0, 1]), 1);
    let sp = STACK_BASE + STACK_SIZE - 256;
    v.k("SetLastError", &[77]);
    assert_eq!(v.call("user32.dll", "GetUpdateRect", &[hwnd, sp, 1]), 1);
    assert_eq!(v.cpu.eip, 0);
    assert_eq!(v.cpu.esp(), sp + 16);
    assert_eq!(v.state.last_error, 77);
    // GetUpdateRect retains the update region and does not invoke erase.
    assert_eq!(v.call("user32.dll", "GetUpdateRect", &[hwnd, 0, 0]), 1);
}

#[test]
fn painting_contract_pe32_preserves_validation_precedence_and_faulting_erase_commit() {
    let mut v = Vm::new();
    assert_eq!(v.call("user32.dll", "RedrawWindow", &[1, 0x30000, 1, 0]), 0);
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.call("user32.dll", "BeginPaint", &[1, 0x30000]), 0);
    assert_eq!(v.state.last_error, 1400);
    let procedure = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, procedure);
    assert_eq!(v.call("user32.dll", "InvalidateRect", &[hwnd, 0, 1]), 1);
    assert!(v
        .try_call("user32.dll", "BeginPaint", &[hwnd, 0x30000])
        .is_err());
    assert_eq!(v.call("user32.dll", "GetUpdateRect", &[hwnd, 0, 0]), 1);
    // A valid PAINTSTRUCT is checked first. The region is then consumed
    // before entering WM_ERASEBKGND, even when the guest callback faults.
    v.mem.write_bytes(CODE, &[0x0f, 0x0b]).unwrap();
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-4i32) as u32, CODE],
    );
    v.mem.write_bytes(DATA + 0x800, &[0xa5; 64]).unwrap();
    assert!(v
        .try_call("user32.dll", "BeginPaint", &[hwnd, DATA + 0x800])
        .is_err());
    assert_eq!(v.call("user32.dll", "GetUpdateRect", &[hwnd, 0, 0]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x800).unwrap(), 0xa5a5a5a5);
}

#[test]
fn painting_contract_pe32_keeps_update_rect_and_dc_lifetime_separate() {
    let mut v = Vm::new();
    let procedure = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, procedure);
    assert_eq!(v.call("user32.dll", "InvalidateRect", &[hwnd, 0, 0]), 1);
    let dc = v.call("user32.dll", "BeginPaint", &[hwnd, DATA + 0x800]);
    assert_ne!(dc, 0);
    assert_eq!(v.mem.read_u32(DATA + 0x804).unwrap(), 0);
    assert_eq!(v.call("user32.dll", "GetUpdateRect", &[hwnd, 0, 0]), 0);
    // Ending paint releases this DC; an independently invalidated region
    // survives EndPaint. A repeated EndPaint reports the original error 6.
    assert_eq!(v.call("user32.dll", "InvalidateRect", &[hwnd, 0, 0]), 1);
    assert_eq!(v.call("user32.dll", "EndPaint", &[hwnd, DATA + 0x800]), 1);
    assert_eq!(v.call("user32.dll", "GetUpdateRect", &[hwnd, 0, 0]), 1);
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), u32::MAX);
    assert_eq!(v.call("user32.dll", "EndPaint", &[hwnd, DATA + 0x800]), 0);
    assert_eq!(v.state.last_error, 6);
    assert_eq!(v.call("user32.dll", "ValidateRect", &[hwnd, 0]), 1);
    assert_eq!(
        v.call("user32.dll", "GetUpdateRect", &[hwnd, DATA + 0x900, 0]),
        0
    );
    for offset in [0, 4, 8, 12] {
        assert_eq!(v.mem.read_u32(DATA + 0x900 + offset).unwrap(), 0);
    }
}
