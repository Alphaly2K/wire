//! Preserve original process-bootstrap scalars, layouts and gateway ordering.
use super::*;

#[test]
fn process_startup_pe32_keeps_owned_command_pointers_feature_set_and_providers() {
    let mut v = Vm::new();
    v.state.cmdline_a = 0xdeadbeef;
    v.state.cmdline_w = 0x30000;
    v.k("SetLastError", &[77]);
    assert_eq!(v.k("GetCommandLineA", &[]), 0xdeadbeef);
    assert_eq!(v.k("GetCommandLineW", &[]), 0x30000);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
    for code in [0, 6, 10, 12, 0x10006, u32::MAX] {
        assert_eq!(
            v.k("IsProcessorFeaturePresent", &[code]),
            u32::from(matches!(code, 6 | 10))
        );
        assert_eq!(v.state.last_error, 77);
    }
    for dll in ["kernelbase.dll", "user32.dll"] {
        for name in [
            "GetCommandLineA",
            "GetCommandLineW",
            "GetStartupInfoA",
            "GetStartupInfoW",
            "IsProcessorFeaturePresent",
        ] {
            assert_eq!(v.call(dll, name, &[0x30000]), 0);
            assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
            assert_eq!(v.state.last_error, 120);
            assert!(v
                .log
                .recent
                .back()
                .unwrap()
                .contains("unknown stdcall arity"));
        }
    }
}

#[test]
fn process_startup_pe32_keeps_record_size_fault_and_return_slot_alias() {
    let mut v = Vm::new();
    v.k("SetLastError", &[77]);
    for name in ["GetStartupInfoA", "GetStartupInfoW"] {
        v.mem.write_bytes(DATA, &[0xa5; 72]).unwrap();
        assert_eq!(v.k(name, &[DATA]), 0);
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 68);
        let mut tail = [0xff; 64];
        v.mem.read_bytes(DATA + 4, &mut tail).unwrap();
        assert_eq!(tail, [0; 64]);
        assert_eq!(v.mem.read_u32(DATA + 68).unwrap(), 0xa5a5a5a5);
        assert_eq!(v.state.last_error, 77);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 8);
        let calls = v.log.call_count;
        v.mem.write_bytes(DATA + 0xffc0, &[0xa5; 64]).unwrap();
        assert!(v.try_call("kernel32.dll", name, &[DATA + 0xffc0]).is_err());
        let mut unchanged = [0; 64];
        v.mem.read_bytes(DATA + 0xffc0, &mut unchanged).unwrap();
        assert_eq!(unchanged, [0xa5; 64]);
        assert_eq!(v.log.call_count, calls);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256);
        assert_eq!(v.state.last_error, 77);
    }
    let sp = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.k("GetStartupInfoW", &[sp]), 0);
    assert_eq!(v.cpu.eip, 68);
    assert_eq!(v.cpu.esp(), sp + 8);
    assert!(v
        .log
        .recent
        .back()
        .unwrap()
        .ends_with(" ; zeroed STARTUPINFO"));
}
