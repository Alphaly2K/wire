//! Win64 display-specific provider scope, direct status, packed POINT and faults.
use super::*;

#[test]
fn display_contract_win64_keeps_static_kernel_dynamic_user_and_metrics_lookup_gap() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "EnumDisplayDevicesA",
            "EnumDisplayDevicesW",
            "EnumDisplaySettingsA",
            "EnumDisplaySettingsW",
            "EnumDisplaySettingsExA",
            "EnumDisplaySettingsExW",
            "EnumDisplayMonitors",
            "MonitorFromPoint",
            "MonitorFromRect",
            "MonitorFromWindow",
            "GetMonitorInfoA",
            "GetMonitorInfoW",
            "GetDisplayConfigBufferSizes",
            "QueryDisplayConfig",
            "DisplayConfigGetDeviceInfo",
        ] {
            assert!(
                wire::x64::runtime::import_supported(dll, name),
                "{dll}!{name}"
            );
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text]
                ) != 0,
                dll == "user32.dll",
                "{dll}!{name}"
            );
        }
        assert!(wire::x64::runtime::import_supported(
            dll,
            "GetSystemMetrics"
        ));
        mem.write(text, b"GetSystemMetrics\0").unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "GetProcAddress",
                &[module, text]
            ),
            0,
            "metrics is absent from the original dynamic export list: {dll}"
        );
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                dll,
                "GetSystemMetrics",
                &[0xabcdef01_00000000]
            ),
            1024
        );
        let gateway = rt.register(dll, "GetSystemMetrics");
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "GetProcAddress",
                &[module, text]
            ),
            gateway,
            "a previously bound static gateway remains discoverable: {dll}"
        );
    }
}

#[test]
fn display_contract_win64_keeps_monitor_checks_and_ccd_direct_error_status() {
    let (mut mem, mut cpu, mut rt) = startup();
    let call =
        |mem: &mut Memory,
         cpu: &mut Cpu64,
         rt: &mut wire::x64::runtime::Runtime64,
         name: &str,
         args: &[u64]| startup_dll_call(mem, cpu, rt, "user32.dll", name, args);
    let monitor = call(&mut mem, &mut cpu, &mut rt, "MonitorFromPoint", &[0, 0]);
    assert!(monitor > u32::MAX as u64);
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "MonitorFromPoint",
            &[u64::MAX, 0]
        ),
        0
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "MonitorFromPoint",
            &[u64::MAX, 0xabcdef01_00000002]
        ),
        monitor
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "MonitorFromRect", &[1, 3]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "MonitorFromWindow",
            &[1u64 << 32, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        1400
    );
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[77]);
    for (name, args) in [
        ("GetDisplayConfigBufferSizes", vec![0, 1, 2]),
        ("QueryDisplayConfig", vec![0, 1, 2, 3, 4, 5]),
        ("DisplayConfigGetDeviceInfo", vec![1]),
    ] {
        assert_eq!(call(&mut mem, &mut cpu, &mut rt, name, &args), 50);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            77
        );
    }
}

#[test]
fn display_contract_win64_output_fault_keeps_prefix_frame_and_api_counter() {
    let (mut mem, mut cpu, mut rt) = startup();
    let output = STACK + 4096 - 124;
    mem.write(output, &[0xa5; 156]).unwrap();
    mem.write_int(output + 36, 2, 156).unwrap();
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    cpu.regs[1] = 0;
    cpu.regs[2] = 0;
    cpu.regs[8] = output;
    let before = (cpu.rip, cpu.regs[4], rt.api_calls);
    assert!(rt
        .dispatch("user32.dll", "EnumDisplaySettingsA", &mut cpu, &mut mem)
        .is_err());
    assert_eq!((cpu.rip, cpu.regs[4], rt.api_calls), before);
    assert_eq!(mem.read_int(output, 4).unwrap(), 0xa5a5a5a5);
}

#[test]
fn configured_desktop_win64_is_consistent_across_metrics_modes_monitors_and_cursor_clip() {
    let (mut mem, mut cpu, mut rt) = startup();
    assert!(rt.configure_desktop(0, 1080).is_err());
    assert!(rt.configure_desktop(8193, 1080).is_err());
    rt.configure_desktop(1920, 1080).unwrap();
    let data = STACK + 0x100;
    for (index, expected) in [(0, 1920), (1, 1080), (78, 1920), (79, 1080)] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "user32.dll",
                "GetSystemMetrics",
                &[index]
            ),
            expected
        );
    }
    for (name, size_at, size, bits) in [
        ("EnumDisplaySettingsA", 36, 156, 104),
        ("EnumDisplaySettingsW", 68, 220, 168),
    ] {
        mem.write_int(data + size_at, 2, size).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "user32.dll",
                name,
                &[0, u32::MAX as u64, data]
            ),
            1
        );
        assert_eq!(mem.read_int(data + bits + 4, 4).unwrap(), 1920);
        assert_eq!(mem.read_int(data + bits + 8, 4).unwrap(), 1080);
    }
    let point = 1800 | (1000u64 << 32);
    let monitor = startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "user32.dll",
        "MonitorFromPoint",
        &[point, 0],
    );
    assert_ne!(monitor, 0);
    mem.write_int(data, 4, 104).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "GetMonitorInfoW",
            &[monitor, data]
        ),
        1
    );
    for offset in [12, 28] {
        assert_eq!(mem.read_int(data + offset, 4).unwrap(), 1920);
    }
    for offset in [16, 32] {
        assert_eq!(mem.read_int(data + offset, 4).unwrap(), 1080);
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "GetClipCursor",
            &[data]
        ),
        1
    );
    assert_eq!(mem.read_int(data + 8, 4).unwrap(), 1920);
    assert_eq!(mem.read_int(data + 12, 4).unwrap(), 1080);
    let dc = startup_dll_call(&mut mem, &mut cpu, &mut rt, "user32.dll", "GetDC", &[0]);
    assert_ne!(dc, 0);
    for (index, expected) in [
        (8, 1920),
        (10, 1080),
        (118, 1920),
        (117, 1080),
        (88, 96),
        (90, 96),
    ] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "gdi32.dll",
                "GetDeviceCaps",
                &[dc, index]
            ),
            expected
        );
    }
    assert!(rt.configure_desktop(1280, 720).is_err());
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "GetSystemMetrics",
            &[0]
        ),
        1920
    );
}
