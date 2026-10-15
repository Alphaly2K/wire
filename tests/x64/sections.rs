//! Section gateway widths, rejection and fault order before catalog migration.
use super::*;

#[test]
fn section_contract_baseline_keeps_win64_widths_and_validation_order() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let high = 0xface0000_00000000;
    for name in ["CreateFileMappingA", "CreateFileMappingW"] {
        // DWORD protection and size halves truncate, while HANDLE does not sign extend.
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                name,
                &[0xffff_ffff, 0, high | 2, high, high, 0]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            6
        );
        // Named/pagefile rejection does not dereference the nonzero name pointer.
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                name,
                &[u64::MAX, 0, high | 2, high, high, u64::MAX]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            50
        );
    }
    for (name, count) in [("MapViewOfFile", 5), ("MapViewOfFileEx", 6)] {
        let args = [0x180000, high | 4, high, high, 0, 0];
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, name, &args[..count]),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            6
        );
    }
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MapViewOfFileEx",
            &[0x180000, high | 4, high, high, 0, 1]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        1132
    );
    for (name, count) in [("FlushViewOfFile", 2), ("UnmapViewOfFile", 1)] {
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, name, &[high, 0][..count]),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            487
        );
    }
    for name in [
        "CreateFileMappingA",
        "CreateFileMappingW",
        "MapViewOfFile",
        "MapViewOfFileEx",
        "UnmapViewOfFile",
        "FlushViewOfFile",
    ] {
        assert!(!wire::x64::runtime::import_supported("user32.dll", name));
    }
}

#[test]
fn section_contract_baseline_keeps_win64_absence_and_faults_uncommitted() {
    let (mut mem, mut cpu, mut runtime) = startup();
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll"] {
        for name in ["OpenFileMappingA", "OpenFileMappingW"] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
            let before = cpu.clone();
            let calls = runtime.api_calls;
            assert!(runtime.dispatch(dll, name, &mut cpu, &mut mem).is_err());
            assert_eq!(cpu, before);
            assert_eq!(runtime.api_calls, calls);
        }
    }
    mem.write_int(wire::x64::runtime::TEB + 0x68, 4, 0x55)
        .unwrap();
    cpu.regs[1] = u64::MAX;
    cpu.regs[2] = STACK + 8192; // Invalid SECURITY_ATTRIBUTES before pagefile rejection.
    cpu.regs[8] = 2;
    cpu.regs[9] = 0;
    mem.write_int(cpu.regs[4] + 40, 8, 0).unwrap();
    mem.write_int(cpu.regs[4] + 48, 8, 0).unwrap();
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("kernel32.dll", "CreateFileMappingW", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(
        mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(),
        0x55
    );
    cpu.regs[4] = STACK + 8192; // Invalid return frame precedes the FlushViewOfFile error.
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "FlushViewOfFile", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(
        mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(),
        0x55
    );
}
