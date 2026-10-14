//! Known PE32 ABI is distinct from the existing Win64 identity implementation.
#[allow(dead_code)]
#[path = "support/x86.rs"]
mod x86;

#[test]
fn pe32_identity_keeps_known_abi_unimplemented_outputs_and_dll_scopes() {
    let mut v = x86::Vm::new();
    for (dll, name, extended) in [
        ("kernel32.dll", "GetComputerNameA", false),
        ("kernel32.dll", "GetComputerNameW", false),
        ("kernel32.dll", "GetComputerNameExA", true),
        ("kernel32.dll", "GetComputerNameExW", true),
        ("kernel32.dll", "GetUserNameA", false),
        ("kernel32.dll", "GetUserNameW", false),
        ("advapi32.dll", "GetUserNameA", false),
        ("advapi32.dll", "GetUserNameW", false),
    ] {
        assert!(wire::api::native_export(dll, name));
        v.mem.write_bytes(x86::DATA, &[0xcc; 32]).unwrap();
        v.mem.write_u32(x86::DATA + 0x100, 16).unwrap();
        let args = if extended {
            vec![3, x86::DATA, x86::DATA + 0x100]
        } else {
            vec![x86::DATA, x86::DATA + 0x100]
        };
        assert_eq!(v.call(dll, name, &args), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.mem.read_u32(x86::DATA).unwrap(), 0xcccccccc);
        assert_eq!(v.mem.read_u32(x86::DATA + 0x100).unwrap(), 16);
        assert_eq!(
            v.cpu.esp(),
            wire::loader::STACK_BASE + wire::loader::STACK_SIZE - 252
                + if extended { 12 } else { 8 }
        );
        assert!(!wire::api::native_export("user32.dll", name));
        assert!(!wire::api::native_export("kernelbase.dll", name));
    }
}
