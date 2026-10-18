//! Independent fixed desktop values, ABI-only settings and unknown DWM ABI.
use super::*;

#[test]
fn desktop_contract_pe32_keeps_fixed_values_and_abi_only_settings() {
    let mut v = Vm::new();
    v.k("SetLastError", &[77]);
    assert_eq!(v.call("user32.dll", "GetDoubleClickTime", &[]), 500);
    assert_eq!(
        v.call("user32.dll", "GetDialogBaseUnits", &[]),
        8 | (16 << 16)
    );
    assert_eq!(v.state.last_error, 77);
    for name in ["SystemParametersInfoA", "SystemParametersInfoW"] {
        v.mem.write_u32(DATA, 0xa5a5a5a5).unwrap();
        assert_eq!(v.call("user32.dll", name, &[0x46, 0, DATA, 0]), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 20);
    }
}

#[test]
fn desktop_contract_pe32_keeps_dwm_unknown_abi_and_zero_cleanup() {
    let mut v = Vm::new();
    for (name, args) in [
        ("DwmIsCompositionEnabled", vec![DATA]),
        ("DwmGetWindowAttribute", vec![1, 9, DATA, 16]),
    ] {
        v.mem.write_u32(DATA, 0xa5a5a5a5).unwrap();
        assert_eq!(v.call("dwmapi.dll", name, &args), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
    }
}
