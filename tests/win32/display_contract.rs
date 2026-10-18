//! Independent PE32 display profile, broad monitor lookup and failure policy.
use super::*;

#[test]
fn display_contract_pe32_keeps_metrics_monitor_lookup_and_packed_point_stack() {
    let mut v = Vm::new();
    for (metric, expected) in [
        (0, 1024),
        (1, 768),
        (4, 23),
        (32, 4),
        (43, 3),
        (83, 2),
        (u32::MAX, 0),
    ] {
        assert_eq!(
            v.call("user32.dll", "GetSystemMetrics", &[metric]),
            expected
        );
    }
    v.k("SetLastError", &[77]);
    assert_eq!(
        v.call(
            "user32.dll",
            "MonitorFromPoint",
            &[u32::MAX, u32::MAX, u32::MAX]
        ),
        0x40000001
    );
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 16);
    assert_eq!(
        v.call("user32.dll", "MonitorFromRect", &[1, u32::MAX]),
        0x40000001
    );
    assert_eq!(
        v.call("user32.dll", "MonitorFromWindow", &[1, u32::MAX]),
        0x40000001
    );
    assert_eq!(v.state.last_error, 77);
}

#[test]
fn display_contract_pe32_keeps_fault_to_error_mapping_and_abi_only_ccd() {
    let mut v = Vm::new();
    let output = DATA + 0x10000 - 124;
    v.mem.write_bytes(output, &[0xa5; 124]).unwrap();
    v.mem.write_u16(output + 36, 156).unwrap();
    assert_eq!(
        v.call("user32.dll", "EnumDisplaySettingsA", &[0, 0, output]),
        0
    );
    assert_eq!(v.state.last_error, 87);
    assert_eq!(v.mem.read_u32(output).unwrap(), 0xa5a5a5a5);
    for (name, args) in [
        ("GetDisplayConfigBufferSizes", vec![0, DATA, DATA + 4]),
        (
            "QueryDisplayConfig",
            vec![0, DATA, DATA + 4, DATA + 8, DATA + 12, DATA + 16],
        ),
        ("DisplayConfigGetDeviceInfo", vec![DATA]),
    ] {
        v.mem.write_bytes(DATA, &[0xa5; 32]).unwrap();
        v.k("SetLastError", &[77]);
        assert_eq!(v.call("user32.dll", name, &args), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
        assert_eq!(
            v.cpu.esp(),
            STACK_BASE + STACK_SIZE - 256 + 4 * (args.len() as u32 + 1)
        );
    }
}

#[test]
fn display_contract_pe32_keeps_adapter_only_enumeration_and_monitor_output() {
    let mut v = Vm::new();
    v.mem.write_u32(DATA, 424).unwrap();
    assert_eq!(
        v.call("user32.dll", "EnumDisplayDevicesA", &[0, 0, DATA, 0]),
        1
    );
    v.mem
        .write_bytes(DATA + 0x400, b"\\\\.\\DISPLAY1\0")
        .unwrap();
    v.k("SetLastError", &[77]);
    assert_eq!(
        v.call(
            "user32.dll",
            "EnumDisplayDevicesA",
            &[DATA + 0x400, 0, DATA, 0]
        ),
        0
    );
    assert_eq!(v.state.last_error, 77);
    for (name, size) in [("GetMonitorInfoA", 72), ("GetMonitorInfoW", 104)] {
        v.mem.write_u32(DATA, size).unwrap();
        assert_eq!(v.call("user32.dll", name, &[0x40000001, DATA]), 1);
        assert_eq!(v.mem.read_u32(DATA + 12).unwrap(), 1024);
        assert_eq!(v.mem.read_u32(DATA + 16).unwrap(), 768);
    }
}
