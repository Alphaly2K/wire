//! PE32 Toolhelp currently has a known ABI and an unavailable implementation.
use super::*;

#[test]
fn snapshot_abi_only_win32_keeps_cleanup_error_outputs_and_unknown_provider_policy() {
    let mut v = Vm::new();
    let sp = STACK_BASE + STACK_SIZE - 256;
    for name in [
        "CreateToolhelp32Snapshot",
        "Process32First",
        "Process32Next",
        "Process32FirstW",
        "Process32NextW",
        "Thread32First",
        "Thread32Next",
    ] {
        for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll"] {
            v.mem.write_u32(DATA, 0xa5a5a5a5).unwrap();
            v.state.last_error = 77;
            assert_eq!(v.call(dll, name, &[0xdead0000, DATA]), 0);
            assert_eq!(v.state.last_error, 120);
            assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
            assert_eq!(v.cpu.esp(), sp + if dll == "kernel32.dll" { 12 } else { 4 });
            assert_eq!(v.cpu.eip, RET);
        }
    }
}
