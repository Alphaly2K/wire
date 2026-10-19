//! Token APIs retain their old unavailable result and provider-specific ABI.
use super::*;

#[test]
fn token_abi_only_win32_preserves_provider_cleanup_outputs_and_no_pointer_reads() {
    let mut v = Vm::new();
    let sp = STACK_BASE + STACK_SIZE - 256;
    for (name, args) in [
        ("OpenProcessToken", vec![u32::MAX, 8, DATA]),
        ("OpenThreadToken", vec![u32::MAX - 1, 8, 1, DATA]),
        ("GetTokenInformation", vec![1, 20, DATA, 4, DATA + 4]),
    ] {
        for dll in [
            "advapi32.dll",
            "kernel32.dll",
            "kernelbase.dll",
            "user32.dll",
        ] {
            v.mem.write_u32(DATA, 0xa5a5a5a5).unwrap();
            v.mem.write_u32(DATA + 4, 0x5a5a5a5a).unwrap();
            v.state.last_error = 77;
            assert_eq!(v.call(dll, name, &args), 0);
            assert_eq!(v.state.last_error, 120);
            assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
            assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 0x5a5a5a5a);
            let known =
                dll == "advapi32.dll" || dll == "kernel32.dll" && name != "GetTokenInformation";
            assert_eq!(
                v.cpu.esp(),
                sp + 4 + if known { args.len() as u32 * 4 } else { 0 }
            );
            assert_eq!(v.cpu.eip, RET);
            assert_eq!(v.call(dll, name, &vec![1; args.len()]), 0);
            assert_eq!(v.state.last_error, 120);
        }
    }
}
