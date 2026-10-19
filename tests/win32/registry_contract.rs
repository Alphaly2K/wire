//! PE32 registry subset and Wine fallback ABI, independent of catalog metadata.
use super::*;

#[test]
fn registry_win32_keeps_subset_outputs_last_error_and_provider_specific_cleanup() {
    let mut v = Vm::new();
    let sp = STACK_BASE + STACK_SIZE - 256;
    v.mem.write_bytes(DATA, b"Software\\Missing\0").unwrap();
    for (name, slots, open, query, close, kernel_abi) in [
        ("RegOpenKeyA", 3, true, false, false, false),
        ("RegOpenKeyW", 3, true, false, false, false),
        ("RegOpenKeyExA", 5, true, false, false, true),
        ("RegOpenKeyExW", 5, true, false, false, true),
        ("RegQueryValueA", 4, false, true, false, false),
        ("RegQueryValueW", 4, false, true, false, false),
        ("RegQueryValueExA", 6, false, true, false, true),
        ("RegQueryValueExW", 6, false, true, false, true),
        ("RegCloseKey", 1, false, false, true, true),
        ("RegCreateKeyA", 3, false, false, false, false),
        ("RegCreateKeyW", 3, false, false, false, false),
        ("RegCreateKeyExA", 9, false, false, false, true),
        ("RegCreateKeyExW", 9, false, false, false, true),
        ("RegSetValueExA", 6, false, false, false, true),
        ("RegSetValueExW", 6, false, false, false, true),
        ("RegDeleteValueA", 2, false, false, false, true),
        ("RegDeleteValueW", 2, false, false, false, true),
    ] {
        for dll in [
            "advapi32.dll",
            "kernel32.dll",
            "kernelbase.dll",
            "user32.dll",
        ] {
            let mut args = vec![DATA; slots];
            args[0] = 0x80000001;
            if open {
                args[if slots == 5 { 4 } else { 2 }] = DATA + 0x100;
            }
            v.mem.write_u32(DATA + 0x100, 0xa5a5a5a5).unwrap();
            v.k("SetLastError", &[77]);
            let implemented = dll == "advapi32.dll" && (open || query || close);
            assert_eq!(
                v.call(dll, name, &args),
                if implemented && !close { 2 } else { 0 },
                "{dll}!{name}"
            );
            assert_eq!(v.state.last_error, if implemented { 77 } else { 120 });
            assert_eq!(
                v.mem.read_u32(DATA + 0x100).unwrap(),
                if implemented && open { 0 } else { 0xa5a5a5a5 }
            );
            assert_eq!(
                v.cpu.esp(),
                sp + 4
                    + if dll == "advapi32.dll" || (dll == "kernel32.dll" && kernel_abi) {
                        slots as u32 * 4
                    } else {
                        0
                    }
            );
            assert_eq!(v.cpu.eip, RET);
        }
    }
}
