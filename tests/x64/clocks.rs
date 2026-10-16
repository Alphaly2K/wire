//! Clock ABI and return-frame baselines independent of catalog declarations.
use super::*;

#[test]
fn multimedia_keeps_win64_capacity_dword_saved_frame_and_lookup_scope() {
    let (mut mem, mut cpu, mut r) = startup();
    let output = STACK + 0x200;
    let sp = STACK + 0x1f88;
    let error = wire::x64::runtime::TEB + 0x68;
    mem.write_int(error, 4, 77).unwrap();
    for period in 1..=256 {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut r,
                "winmm.dll",
                "timeBeginPeriod",
                &[period]
            ),
            0
        );
    }
    for (name, period, expected) in [
        ("timeBeginPeriod", 257, 97),
        ("timeBeginPeriod", 0, 97),
        ("timeBeginPeriod", 0x100000001, 0),
        ("timeEndPeriod", 1, 0),
        ("timeBeginPeriod", 257, 97),
        ("timeEndPeriod", 1, 0),
        ("timeEndPeriod", 1, 97),
        ("timeBeginPeriod", 257, 0),
    ] {
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut r, "winmm.dll", name, &[period]),
            expected
        );
    }
    for dll in ["kernel32.dll", "kernelbase.dll", "winmm.dll"] {
        for name in ["timeGetDevCaps", "timeBeginPeriod", "timeEndPeriod"] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            assert!(!wire::x64::runtime::import_supported("user32.dll", name));
        }
        mem.write(output, &[0xa5; 16]).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut r,
                dll,
                "timeGetDevCaps",
                &[output, 0x100000007]
            ),
            97
        );
        assert_eq!(mem.read_int(output, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut r, dll, "timeGetDevCaps", &[sp, 8]),
            0
        );
        assert_eq!(cpu.rip, BASE);
        assert_eq!(mem.read_int(sp, 8).unwrap(), 0xffffffff00000001);
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut r, dll, "timeGetDevCaps", &[0, 8]),
            97
        );
        cpu.regs[4] = sp;
        mem.write_int(sp, 8, BASE).unwrap();
        cpu.regs[1] = 0xface000000020000;
        cpu.regs[2] = 8;
        let before = cpu.clone();
        let calls = r.api_calls;
        assert!(r
            .dispatch(dll, "timeGetDevCaps", &mut cpu, &mut mem)
            .is_err());
        assert_eq!(cpu, before);
        assert_eq!(r.api_calls, calls);
        r.register_provider(dll);
        let name: Vec<_> = dll
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        mem.write(output, &name).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut r, "GetModuleHandleW", &[output]);
        assert_ne!(module, 0);
        for name in ["timeGetDevCaps", "timeBeginPeriod", "timeEndPeriod"] {
            mem.write(output + 64, format!("{name}\0").as_bytes())
                .unwrap();
            let address = startup_call(
                &mut mem,
                &mut cpu,
                &mut r,
                "GetProcAddress",
                &[module, output + 64],
            );
            assert_eq!(address != 0, dll == "winmm.dll", "{dll}!{name}");
        }
    }
    // Missing kernel lookups set 127; ordinary MMRESULT calls preserve LastError.
    mem.write_int(error, 4, 77).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut r,
            "winmm.dll",
            "timeEndPeriod",
            &[0]
        ),
        97
    );
    assert_eq!(mem.read_int(error, 4).unwrap(), 77);
}

#[test]
fn calendar_keeps_win64_saved_frame_compare_carrier_and_checked_output_faults() {
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        let (mut mem, mut cpu, mut runtime) = startup();
        let source = STACK + 0x100;
        let output = STACK + 0x200;
        let sp = STACK + 0x1f88;
        let error = wire::x64::runtime::TEB + 0x68;
        mem.write_int(error, 4, 77).unwrap();
        for (a, b, expected) in [(0, u64::MAX, u32::MAX as u64), (9, 9, 0), (u64::MAX, 0, 1)] {
            mem.write_int(source, 8, a).unwrap();
            mem.write_int(output, 8, b).unwrap();
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    "CompareFileTime",
                    &[source, output]
                ),
                expected
            );
        }
        let calendar: Vec<_> = [1970u16, 1, 0, 1, 0, 0, 0, 0]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        mem.write(source, &calendar).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "SystemTimeToFileTime",
                &[source, sp]
            ),
            1
        );
        assert_eq!(cpu.rip, BASE);
        assert_eq!(mem.read_int(sp, 8).unwrap(), 116444736000000000);
        for name in [
            "GetSystemTime",
            "GetLocalTime",
            "GetSystemTimePreciseAsFileTime",
        ] {
            mem.write(output, &[0xa5; 24]).unwrap();
            assert_eq!(
                startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, name, &[output]),
                0
            );
            if name == "GetSystemTimePreciseAsFileTime" {
                assert!(mem.read_int(output, 8).unwrap() > 116444736000000000);
                assert_eq!(mem.read_int(output + 8, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
            } else {
                assert!(mem.read_int(output, 2).unwrap() >= 2026);
                assert_eq!(mem.read_int(output + 16, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
            }
            assert_eq!(mem.read_int(error, 4).unwrap(), 77);
        }
        for (name, args) in [
            ("GetSystemTime", vec![0]),
            ("GetLocalTime", vec![0]),
            ("GetSystemTimePreciseAsFileTime", vec![0]),
            ("SystemTimeToFileTime", vec![source, 0]),
            ("CompareFileTime", vec![source, 0]),
        ] {
            cpu.regs[4] = sp;
            mem.write_int(sp, 8, BASE).unwrap();
            for (i, value) in args.into_iter().enumerate() {
                cpu.regs[[1, 2, 8, 9][i]] = value;
            }
            let before = cpu.clone();
            let calls = runtime.api_calls;
            assert!(
                runtime.dispatch(dll, name, &mut cpu, &mut mem).is_err(),
                "{name}"
            );
            assert_eq!(cpu, before, "{name}");
            assert_eq!(runtime.api_calls, calls, "{name}");
            assert_eq!(mem.read_int(error, 4).unwrap(), 77, "{name}");
        }
        assert!(!wire::x64::runtime::import_supported(
            dll,
            "FileTimeToLocalFileTime"
        ));
    }
}

#[test]
fn clocks_keep_win64_hot_output_alias_full_pointer_and_fault_preflight() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let sp = STACK + 0x1f88;
    mem.map(0x20000, 4096, READ | WRITE).unwrap();
    mem.write_int(0x20000, 8, 0x55667788).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "QueryPerformanceFrequency",
            &[sp]
        ),
        1
    );
    assert_eq!(mem.read_int(sp, 8).unwrap(), 10_000_000);
    let teb = wire::x64::runtime::TEB;
    mem.write_int(teb + 0x68, 4, 0x55).unwrap();
    for name in [
        "QueryPerformanceCounter",
        "QueryPerformanceFrequency",
        "GetSystemTimeAsFileTime",
    ] {
        for (rsp, pointer) in [(STACK + 8192, STACK + 0x100), (sp, 0xface0000_00020000)] {
            mem.write_int(sp, 8, BASE).unwrap();
            mem.write_int(STACK + 0x100, 8, 0x11223344).unwrap();
            cpu.regs[4] = rsp;
            cpu.regs[1] = pointer;
            let before = cpu.clone();
            let calls = runtime.api_calls;
            assert!(runtime
                .dispatch("kernelbase.dll", name, &mut cpu, &mut mem)
                .is_err());
            assert_eq!(cpu, before);
            assert_eq!(runtime.api_calls, calls);
            assert_eq!(mem.read_int(teb + 0x68, 4).unwrap(), 0x55);
            assert_eq!(mem.read_int(STACK + 0x100, 8).unwrap(), 0x11223344);
            assert_eq!(mem.read_int(0x20000, 8).unwrap(), 0x55667788);
        }
    }
}

#[test]
fn clocks_keep_win64_multimedia_static_and_dynamic_provider_difference() {
    let (mut mem, mut cpu, mut runtime) = startup();
    runtime.register_provider("kernel32.dll");
    for dll in ["kernel32.dll", "kernelbase.dll", "winmm.dll"] {
        assert!(wire::x64::runtime::import_supported(dll, "timeGetTime"));
        let tick = startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, "timeGetTime", &[]);
        assert!(tick < 60_000);
    }
    assert!(!wire::x64::runtime::import_supported(
        "user32.dll",
        "timeGetTime"
    ));
    // Legacy static kernel imports are admitted, but kernel GetProcAddress
    // deliberately excludes multimedia exports.
    let symbol = STACK + 0x100;
    mem.write(symbol, b"timeGetTime\0").unwrap();
    let module_name = STACK + 0x200;
    let name: Vec<u8> = "kernel32.dll"
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    mem.write(module_name, &name).unwrap();
    let kernel = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetModuleHandleW",
        &[module_name],
    );
    assert_ne!(kernel, 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[kernel, symbol]
        ),
        0
    );
}

#[test]
fn clocks_keep_win64_delay_resolution_and_general_output_submission() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let handle = STACK + 0x200;
    let iat = STACK + 0x208;
    runtime
        .install_delays(
            &mut mem,
            STACK,
            &[wire::x64::pe::Delay64 {
                dll: "kernel32.dll".into(),
                module_handle_rva: 0x200,
                functions: vec![wire::pe::ImportFunc {
                    iat_rva: 0x208,
                    name: Some("QueryPerformanceFrequency".into()),
                    ordinal: None,
                }],
            }],
        )
        .unwrap();
    let gateway = mem.read_int(iat, 8).unwrap();
    let sp = STACK + 0x1f88;
    mem.write_int(wire::x64::runtime::TEB + 0x68, 4, 0x55)
        .unwrap();
    cpu.rip = gateway;
    cpu.regs[1] = sp;
    assert_eq!(
        runtime
            .dispatch(
                "kernel32.dll",
                "QueryPerformanceFrequency",
                &mut cpu,
                &mut mem
            )
            .unwrap(),
        wire::x64::runtime::Control::Returned
    );
    assert_eq!(cpu.rip, BASE);
    assert_eq!(cpu.regs[4], sp + 8);
    assert_eq!(cpu.regs[0], 1);
    assert_eq!(mem.read_int(sp, 8).unwrap(), 10_000_000);
    assert_ne!(mem.read_int(handle, 8).unwrap(), 0);
    assert_eq!(mem.read_int(iat, 8).unwrap(), gateway);
    assert_eq!(
        mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(),
        0x55
    );
    assert_eq!(runtime.api_calls, 1);
}
