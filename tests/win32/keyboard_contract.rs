//! Existing keyboard API policy, fixed independently of the next catalog.
use super::*;

#[test]
fn keyboard_contract_pe32_layout_helpers_remain_abi_only_without_output_writes() {
    let mut v = Vm::new();
    for (name, arity) in [
        ("GetKeyboardLayout", 1),
        ("GetKeyboardLayoutList", 2),
        ("GetKeyboardLayoutNameA", 1),
        ("GetKeyboardLayoutNameW", 1),
        ("GetKeyboardType", 1),
        ("MapVirtualKeyA", 2),
        ("MapVirtualKeyW", 2),
        ("MapVirtualKeyExA", 3),
        ("MapVirtualKeyExW", 3),
        ("GetKeyNameTextA", 3),
        ("GetKeyNameTextW", 3),
        ("ToUnicode", 6),
        ("ToUnicodeEx", 7),
    ] {
        v.mem.write_bytes(DATA, &[0xaa; 256]).unwrap();
        v.state.last_error = 123;
        assert_eq!(v.call("user32.dll", name, &vec![DATA; arity]), 0, "{name}");
        assert_eq!(v.state.last_error, 120, "{name}");
        assert_eq!(
            v.cpu.esp(),
            STACK_BASE + STACK_SIZE - 256 + 4 * (arity as u32 + 1),
            "{name}"
        );
        assert_eq!(v.cpu.eip, RET);
        let mut output = [0; 256];
        v.mem.read_bytes(DATA, &mut output).unwrap();
        assert_eq!(output, [0xaa; 256], "{name}");
    }
}

#[test]
fn keyboard_contract_pe32_state_preserves_last_error_and_reads_return_after_output() {
    let mut v = Vm::new();
    v.k("SetLastError", &[123]);
    for name in ["GetKeyState", "GetAsyncKeyState"] {
        for key in [65, 256, u32::MAX] {
            assert_eq!(v.call("user32.dll", name, &[key]), 0);
            assert_eq!(v.state.last_error, 123);
        }
    }
    let slot = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.call("user32.dll", "GetKeyboardState", &[slot]), 1);
    assert_eq!(v.cpu.eip, 0);
    assert_eq!(v.cpu.esp(), slot + 8);
    assert_eq!(v.state.last_error, 123);
    assert!(v
        .try_call("user32.dll", "GetKeyboardState", &[DATA + 0xff80])
        .is_err());
    assert_eq!(v.state.last_error, 123);
    for name in ["GetKeyState", "GetAsyncKeyState", "GetKeyboardState"] {
        assert_eq!(v.k(name, &[65]), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.cpu.eip, RET);
        assert_eq!(v.cpu.esp(), slot + 4); // Wrong provider retains unknown ABI.
    }
}
