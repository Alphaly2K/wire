//! These PE32 paint APIs are absent from the original Win64 service surface.
#[test]
fn painting_contract_win64_keeps_six_paint_exports_absent() {
    for dll in ["user32.dll", "kernel32.dll", "kernelbase.dll"] {
        for name in [
            "InvalidateRect",
            "ValidateRect",
            "GetUpdateRect",
            "BeginPaint",
            "EndPaint",
            "RedrawWindow",
        ] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
        }
    }
}
