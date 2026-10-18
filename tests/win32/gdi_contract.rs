//! Independent DC ownership, device profile and ABI-only GDI boundaries.
use super::*;

#[test]
fn gdi_contract_pe32_keeps_display_and_compatible_dc_lifetimes() {
    let mut v = Vm::new();
    let screen = v.call("user32.dll", "GetDC", &[0]);
    assert_ne!(screen, 0);
    let compatible = v.call("gdi32.dll", "CreateCompatibleDC", &[screen]);
    assert_ne!(compatible, 0);
    assert_ne!(screen, compatible);
    assert_eq!(v.call("gdi32.dll", "DeleteDC", &[screen]), 0);
    assert_eq!(v.state.last_error, 6);
    assert_eq!(v.call("user32.dll", "ReleaseDC", &[0, compatible]), 0);
    assert_eq!(v.state.last_error, 6);
    assert_eq!(v.call("user32.dll", "ReleaseDC", &[1, screen]), 0);
    assert_eq!(v.call("gdi32.dll", "GetDeviceCaps", &[screen, 88]), 96);
    assert_eq!(v.call("user32.dll", "ReleaseDC", &[0, screen]), 1);
    assert_eq!(v.call("gdi32.dll", "GetDeviceCaps", &[screen, 88]), 0);
    assert_eq!(v.state.last_error, 6);
    assert_eq!(v.call("gdi32.dll", "DeleteDC", &[compatible]), 1);
    assert_eq!(v.call("gdi32.dll", "DeleteDC", &[compatible]), 0);
    assert_eq!(v.call("user32.dll", "GetDC", &[1]), 0);
    assert_eq!(v.state.last_error, 1400);
}

#[test]
fn gdi_contract_pe32_keeps_capability_profile_and_abi_only_pixel_format() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    v.k("SetLastError", &[77]);
    for (index, expected) in [
        (0, 0x300),
        (8, 1024),
        (10, 768),
        (24, u32::MAX),
        (38, 0x2081),
        (40, 0),
        (88, 96),
        (116, 0),
        (u32::MAX, 0),
    ] {
        assert_eq!(v.call("gdi32.dll", "GetDeviceCaps", &[dc, index]), expected);
        assert_eq!(v.state.last_error, 77);
    }
    for (dll, name, argument) in [
        ("gdi32.dll", "GetPixelFormat", dc),
        ("user32.dll", "GetWindowDC", 0),
    ] {
        assert_eq!(v.call(dll, name, &[argument]), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 8);
    }
    assert_eq!(v.call("gdi32.dll", "GetDeviceCaps", &[dc, 12]), 32);
    assert_eq!(v.call("gdi32.dll", "DeleteDC", &[dc]), 1);
}

#[test]
fn gdi_contract_pe32_keeps_stock_objects_and_error_color_sentinel() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    let bitmap = v.call("gdi32.dll", "GetCurrentObject", &[dc, 7]);
    assert_ne!(bitmap, 0);
    assert_eq!(v.call("gdi32.dll", "DeleteObject", &[bitmap]), 0);
    assert_eq!(v.state.last_error, 170);
    assert_eq!(v.call("gdi32.dll", "GetTextColor", &[1]), u32::MAX);
    assert_eq!(v.state.last_error, 6);
    assert_eq!(v.call("gdi32.dll", "GdiFlush", &[]), 1);
    assert_eq!(v.call("gdi32.dll", "DeleteDC", &[dc]), 1);
}
