//! Clock ABI and return-frame baselines independent of catalog declarations.
use super::*;

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
