//! Original update-region and erase-callback commit rules before migration.
use super::*;

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
