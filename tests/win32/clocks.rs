//! Clock gateway behavior fixed before catalog migration.
use super::*;

#[test]
fn calendar_keeps_pe32_public_dispatch_errors_and_abi_only_conversions() {
    let mut v = Vm::new();
    let sp = STACK_BASE + STACK_SIZE - 256;
    v.k("SetLastError", &[77]);
    for name in ["GetSystemTime", "GetLocalTime"] {
        v.mem.write_bytes(DATA, &[0xa5; 24]).unwrap();
        assert_eq!(v.k(name, &[DATA]), 0);
        assert!(v.mem.read_u16(DATA).unwrap() >= 2026);
        assert!((1..=12).contains(&v.mem.read_u16(DATA + 2).unwrap()));
        assert_eq!(v.mem.read_u32(DATA + 16).unwrap(), 0xa5a5a5a5);
        assert_eq!(v.state.last_error, 77);
        assert_eq!(v.k(name, &[sp]), 0);
        assert_eq!(v.cpu.eip, v.mem.read_u32(sp).unwrap());
        assert_eq!(v.cpu.esp(), sp + 8);
        assert_eq!(v.k(name, &[0]), 0);
        assert_eq!(v.state.last_error, 87);
        let mut args = [0; 16];
        args[0] = DATA;
        assert!(
            wire::system::dispatch("KERNEL32.dll", name, &args, &v.mem, &mut v.state).is_none()
        );
        assert!(
            wire::system::dispatch("kernelbase.dll", name, &args, &v.mem, &mut v.state).is_none()
        );
        assert_eq!(
            wire::system::dispatch("kernel32.dll", name, &args, &v.mem, &mut v.state)
                .unwrap()
                .0,
            0
        );
        v.k("SetLastError", &[77]);
    }
    v.mem.write_bytes(DATA, &[0xa5; 192]).unwrap();
    assert_eq!(v.k("GetTimeZoneInformation", &[DATA]), 0);
    assert_eq!(v.state.last_error, 77);
    let mut zone = [0; 192];
    v.mem.read_bytes(DATA, &mut zone).unwrap();
    assert_eq!(zone[..172], [0; 172]);
    assert_eq!(zone[172..], [0xa5; 20]);
    assert_eq!(v.k("GetTimeZoneInformation", &[0]), 0);
    assert_eq!(v.state.last_error, 87);
    for (name, arity) in [
        ("SystemTimeToFileTime", 2),
        ("FileTimeToSystemTime", 2),
        ("CompareFileTime", 2),
        ("FileTimeToLocalFileTime", 2),
        ("SystemTimeToTzSpecificLocalTime", 3),
        ("TzSpecificLocalTimeToSystemTime", 3),
        ("GetSystemTimePreciseAsFileTime", 1),
        ("GetSystemTimes", 3),
    ] {
        v.k("SetLastError", &[77]);
        v.mem.write_bytes(DATA, &[0xa5; 192]).unwrap();
        assert_eq!(
            v.k(name, &[DATA, DATA + 32, DATA + 64][..arity]),
            0,
            "{name}"
        );
        assert_eq!(v.state.last_error, 120, "{name}");
        assert_eq!(v.cpu.esp(), sp + 4 + 4 * arity as u32, "{name}");
        let mut output = [0; 192];
        v.mem.read_bytes(DATA, &mut output).unwrap();
        assert_eq!(output, [0xa5; 192], "{name}");
    }
}

#[test]
fn clocks_keep_pe32_tick_pair_frequency_and_provider_scope() {
    let mut v = Vm::new();
    let sp = STACK_BASE + STACK_SIZE - 256;
    v.state.start =
        std::time::Instant::now() - std::time::Duration::from_millis((1u64 << 32) + 1000);
    v.k("SetLastError", &[0x55]);
    assert!((1000..5000).contains(&v.k("GetTickCount64", &[])));
    assert_eq!(v.cpu.regs[2], 1);
    assert_eq!(v.cpu.esp(), sp + 4);
    v.cpu.regs[2] = 0x12345678;
    for (dll, name) in [
        ("kernel32.dll", "GetTickCount"),
        ("winmm.dll", "timeGetTime"),
    ] {
        assert!((1000..5000).contains(&v.call(dll, name, &[])));
        assert_eq!(v.cpu.regs[2], 0x12345678);
        assert_eq!(v.cpu.esp(), sp + 4);
        assert_eq!(v.state.last_error, 0x55);
    }
    assert_eq!(v.k("QueryPerformanceFrequency", &[DATA]), 1);
    assert_eq!(v.mem.read_u64(DATA).unwrap(), 10_000_000);
    assert_eq!(v.cpu.regs[2], 0x12345678);
    assert_eq!(v.cpu.esp(), sp + 8);
    for name in [
        "GetTickCount",
        "GetTickCount64",
        "QueryPerformanceCounter",
        "QueryPerformanceFrequency",
        "GetSystemTimeAsFileTime",
        "timeGetTime",
    ] {
        assert!(!api::native_export("user32.dll", name));
    }
}

#[test]
fn clocks_keep_pe32_checked_output_fault_and_alias_submission_order() {
    let mut v = Vm::new();
    v.k("SetLastError", &[0x55]);
    v.mem.write_u64(DATA, 0x1122334455667788).unwrap();
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_READONLY)
        .unwrap();
    for name in [
        "QueryPerformanceCounter",
        "QueryPerformanceFrequency",
        "GetSystemTimeAsFileTime",
    ] {
        v.cpu.regs[0] = 0x1234;
        v.cpu.regs[2] = 0x5678;
        assert!(v.try_call("kernel32.dll", name, &[DATA]).is_err());
        assert_eq!((v.cpu.regs[0], v.cpu.regs[2]), (0x1234, 0x5678));
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256);
        assert_eq!(v.mem.read_u64(DATA).unwrap(), 0x1122334455667788);
        assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), 0x55);
    }
    let sp = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.k("QueryPerformanceFrequency", &[sp]), 1);
    assert_eq!(v.cpu.eip, 10_000_000);
    assert_eq!(v.cpu.esp(), sp + 8);
    assert_eq!(v.mem.read_u32(sp + 4).unwrap(), 0);
}
