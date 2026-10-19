//! Legacy SID interfaces have an ABI, but no PE32 implementation.
use super::*;

#[test]
fn sid_abi_only_win32_preserves_all_arities_and_unknown_provider_cleanup() {
    let mut v = Vm::new();
    let sp = STACK_BASE + STACK_SIZE - 256;
    for (name, count) in [
        ("GetSidSubAuthority", 2),
        ("GetSidSubAuthorityCount", 1),
        ("GetSidIdentifierAuthority", 1),
        ("IsValidSid", 1),
        ("GetLengthSid", 1),
        ("CopySid", 3),
        ("EqualSid", 2),
        ("GetSidLengthRequired", 1),
        ("InitializeSid", 3),
    ] {
        for dll in [
            "advapi32.dll",
            "kernel32.dll",
            "kernelbase.dll",
            "user32.dll",
        ] {
            v.mem.write_u32(DATA, 0x12345678).unwrap();
            v.state.last_error = 77;
            assert_eq!(v.call(dll, name, &vec![DATA; count]), 0);
            assert_eq!(v.state.last_error, 120);
            assert_eq!(v.mem.read_u32(DATA).unwrap(), 0x12345678);
            assert_eq!(
                v.cpu.esp(),
                sp + 4
                    + if dll == "advapi32.dll" {
                        count as u32 * 4
                    } else {
                        0
                    }
            );
            assert_eq!(v.cpu.eip, RET);
        }
    }
}
