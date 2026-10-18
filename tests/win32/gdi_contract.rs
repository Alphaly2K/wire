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

#[test]
fn gdi_object_output_keeps_return_slot_alias_and_cross_page_failure_order() {
    let mut v = Vm::new();
    let bitmap = v.call("gdi32.dll", "GetStockObject", &[21]);
    let sp = STACK_BASE + STACK_SIZE - 256;
    v.k("SetLastError", &[99]);
    assert_eq!(v.call("gdi32.dll", "GetObjectW", &[bitmap, 24, sp]), 24);
    // BITMAP.bmType overwrites the return address before the gateway reads it.
    assert_eq!(v.cpu.eip, 0);
    assert_eq!(v.cpu.esp(), sp + 16);
    assert_eq!(v.mem.read_u32(sp + 4).unwrap(), 1);
    assert_eq!(v.state.last_error, 99);

    let output = DATA + 0xfff0;
    v.mem.write_bytes(output, &[0xa5; 16]).unwrap();
    assert_eq!(v.call("gdi32.dll", "GetObjectA", &[bitmap, 24, output]), 0);
    assert_eq!(v.cpu.eip, RET);
    assert_eq!(v.cpu.esp(), sp + 16);
    assert_eq!(v.state.last_error, 87);
    let mut bytes = [0; 16];
    v.mem.read_bytes(output, &mut bytes).unwrap();
    assert_eq!(bytes, [0xa5; 16]);
    let line = v.log.recent.back().unwrap();
    assert!(line.ends_with(" ; GDI error 87"));
    assert!(!line.contains("[UNIMPLEMENTED]"));
}

#[test]
fn gdi_pixel_errors_keep_color_bool_trace_and_wrong_provider_distinctions() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[0, 0, 0]), u32::MAX);
    assert_eq!(v.state.last_error, 6);
    assert!(!v.log.recent.back().unwrap().contains("[UNIMPLEMENTED]"));
    for (name, expected) in [("SetPixel", u32::MAX), ("SetPixelV", 0)] {
        assert_eq!(v.call("gdi32.dll", name, &[dc, 0, 0, 0x01000000]), expected);
        assert_eq!(v.state.last_error, 120);
        assert!(v
            .log
            .recent
            .back()
            .unwrap()
            .contains("[UNIMPLEMENTED] ; GDI error 120"));
    }
    assert_eq!(
        v.call("gdi32.dll", "SetTextColor", &[dc, 0x01000000]),
        u32::MAX
    );
    assert_eq!(v.call("gdi32.dll", "GetTextColor", &[dc]), 0);
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.call("gdi32.dll", "GdiFlush", &[]), 1);
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.call("user32.dll", "GetPixel", &[dc, 0, 0]), 0);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
    assert!(v
        .log
        .recent
        .back()
        .unwrap()
        .ends_with(" ; unknown stdcall arity, assuming cdecl/0-byte cleanup"));
}

#[test]
fn gdi_dib_error_diagnostics_keep_handle_precedence_and_readable_info() {
    let mut v = Vm::new();
    let info = [40u32, 3, (-2i32) as u32, 0x00200001, 0, 0, 0, 0, 0, 0];
    for (i, word) in info.into_iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, word).unwrap();
    }
    v.mem.write_u32(DATA + 0x100, 0x11223344).unwrap();
    assert_eq!(
        v.call(
            "gdi32.dll",
            "CreateDIBSection",
            &[1, DATA, 1, DATA + 0x100, 1, 1]
        ),
        0
    );
    assert_eq!(v.state.last_error, 6);
    assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 0x11223344);
    assert!(v
        .log
        .recent
        .back()
        .unwrap()
        .ends_with(" ; GDI error 6; BITMAPINFO=[28, 3, fffffffe, 200001, 0, 0, 0, 0, 0, 0]"));
    assert_eq!(
        v.call(
            "gdi32.dll",
            "CreateDIBSection",
            &[1, DATA + 0x10000, 1, DATA + 0x100, 1, 1]
        ),
        0
    );
    assert_eq!(v.state.last_error, 6);
    assert!(v.log.recent.back().unwrap().ends_with(" ; GDI error 6"));
}
