//! REGHANDLE and keyword values occupy two PE32 DWORD slots.
use super::*;

#[test]
fn etw_abi_only_win32_preserves_u64_slots_outputs_and_unknown_provider_cleanup() {
    let mut v = Vm::new();
    let sp = STACK_BASE + STACK_SIZE - 256;
    for (name, slots) in [
        ("EventRegister", 4),
        ("EventUnregister", 2),
        ("EventEnabled", 3),
        ("EventProviderEnabled", 5),
        ("EventWrite", 5),
        ("EventWriteString", 6),
        ("EventWriteTransfer", 7),
        ("EventSetInformation", 5),
    ] {
        for dll in [
            "advapi32.dll",
            "kernel32.dll",
            "kernelbase.dll",
            "ntdll.dll",
        ] {
            v.mem.write_u32(DATA, 0x12345678).unwrap();
            v.state.last_error = 77;
            assert_eq!(v.call(dll, name, &vec![DATA; slots]), 0);
            assert_eq!(v.state.last_error, 120);
            assert_eq!(v.mem.read_u32(DATA).unwrap(), 0x12345678);
            assert_eq!(
                v.cpu.esp(),
                sp + 4
                    + if dll == "advapi32.dll" {
                        slots as u32 * 4
                    } else {
                        0
                    }
            );
            assert_eq!(v.cpu.eip, RET);
        }
    }
}
