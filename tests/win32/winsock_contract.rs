//! PE32's limited Winsock ABI is independent of Win64 implementation coverage.
use super::*;

#[test]
fn winsock_preserves_abi_only_names_unknown_ordinals_and_provider_specific_cleanup() {
    let mut vm = Vm::new();
    let stack = STACK_BASE + STACK_SIZE - 256;
    for (name, slots, known) in [
        ("WSAStartup", 2, true),
        ("WSACleanup", 0, true),
        ("socket", 3, true),
        ("closesocket", 1, true),
        ("WSAGetLastError", 0, false),
        ("WSASetLastError", 1, false),
        ("WSAWaitForMultipleEvents", 5, false),
        ("htonl", 1, false),
        ("InetPtonW", 3, false),
        ("#115", 2, false),
        ("#23", 3, false),
    ] {
        for dll in ["ws2_32.dll", "kernel32.dll", "kernelbase.dll"] {
            vm.k("SetLastError", &[77]);
            vm.mem.write_u32(DATA, 0xa5a5a5a5).unwrap();
            let args = vec![DATA; slots];
            assert_eq!(vm.call(dll, name, &args), 0, "{dll}!{name}");
            assert_eq!(vm.state.last_error, 120);
            assert_eq!(vm.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
            assert_eq!(vm.cpu.eip, RET);
            assert_eq!(
                vm.cpu.esp(),
                stack
                    + 4
                    + if dll == "ws2_32.dll" && known {
                        slots as u32 * 4
                    } else {
                        0
                    },
                "{dll}!{name}"
            );
        }
    }
}
