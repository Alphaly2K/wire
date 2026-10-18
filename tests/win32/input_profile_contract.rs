//! Existing absent raw-input/IME facilities keep their known PE32 ABI.
use super::*;

#[test]
fn input_profile_contract_pe32_preserves_abi_only_errors_and_all_output_buffers() {
    let mut v = Vm::new();
    for (dll, name, arity) in [
        ("user32.dll", "RegisterRawInputDevices", 3),
        ("user32.dll", "GetRegisteredRawInputDevices", 3),
        ("user32.dll", "GetRawInputDeviceList", 3),
        ("user32.dll", "GetRawInputBuffer", 3),
        ("user32.dll", "GetRawInputDeviceInfoA", 4),
        ("user32.dll", "GetRawInputDeviceInfoW", 4),
        ("user32.dll", "GetRawInputData", 5),
        ("imm32.dll", "ImmGetContext", 1),
        ("imm32.dll", "ImmReleaseContext", 2),
        ("imm32.dll", "ImmGetCompositionStringA", 4),
        ("imm32.dll", "ImmGetCompositionStringW", 4),
        ("imm32.dll", "ImmSetCompositionStringA", 6),
        ("imm32.dll", "ImmSetCompositionStringW", 6),
        ("imm32.dll", "ImmNotifyIME", 4),
        ("imm32.dll", "ImmGetDefaultIMEWnd", 1),
        ("imm32.dll", "ImmIsIME", 1),
        ("imm32.dll", "ImmGetOpenStatus", 1),
        ("imm32.dll", "ImmSetOpenStatus", 2),
        ("imm32.dll", "ImmAssociateContext", 2),
        ("imm32.dll", "ImmAssociateContextEx", 3),
        ("imm32.dll", "ImmSetCompositionFontW", 2),
        ("imm32.dll", "ImmSetConversionStatus", 3),
        ("imm32.dll", "ImmGetConversionStatus", 3),
        ("imm32.dll", "ImmSetCompositionWindow", 2),
    ] {
        v.mem.write_bytes(DATA, &[0xa5; 256]).unwrap();
        v.k("SetLastError", &[123]);
        assert_eq!(v.call(dll, name, &vec![DATA; arity]), 0, "{name}");
        assert_eq!(v.state.last_error, 120, "{name}");
        assert_eq!(v.cpu.eip, RET, "{name}");
        assert_eq!(
            v.cpu.esp(),
            STACK_BASE + STACK_SIZE - 256 + 4 * (arity as u32 + 1),
            "{name}"
        );
        let mut bytes = [0; 256];
        v.mem.read_bytes(DATA, &mut bytes).unwrap();
        assert_eq!(bytes, [0xa5; 256], "{name}");
    }
}
