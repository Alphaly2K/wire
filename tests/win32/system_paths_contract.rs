//! Existing Wine directory ABI remains a PE32 capability boundary.
use super::*;

#[test]
fn system_paths_remain_abi_only_with_provider_specific_cleanup() {
    let mut vm = Vm::new();
    for name in [
        "GetSystemDirectoryA",
        "GetSystemDirectoryW",
        "GetWindowsDirectoryA",
        "GetWindowsDirectoryW",
        "GetSystemWindowsDirectoryA",
        "GetSystemWindowsDirectoryW",
        "GetTempPathA",
        "GetTempPathW",
        "GetTempPath2A",
        "GetTempPath2W",
    ] {
        for dll in ["kernel32.dll", "kernelbase.dll"] {
            vm.k("SetLastError", &[77]);
            vm.mem.write_u32(DATA, 0xa5a5a5a5).unwrap();
            assert_eq!(vm.call(dll, name, &[DATA, DATA]), 0);
            assert_eq!(vm.state.last_error, 120);
            assert_eq!(vm.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
            assert_eq!(vm.cpu.eip, RET);
            assert_eq!(
                vm.cpu.esp(),
                STACK_BASE + STACK_SIZE - 256 + 4 + if dll == "kernel32.dll" { 8 } else { 0 },
                "{dll}!{name}"
            );
        }
    }
}
