//! DWORD indices, pointer payloads and checked slot lifetime boundaries.
use super::*;

#[test]
fn locals_keep_win64_teb_values_dword_indices_and_invalid_slot_errors() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let teb = wire::x64::runtime::TEB;
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        let slot = startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, "TlsAlloc", &[]);
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "TlsSetValue",
                &[0xfacefeed_00000000 | slot, 0x87654321_abcdef01],
            ),
            1
        );
        assert_eq!(
            mem.read_int(teb + 0x1480 + slot * 8, 8).unwrap(),
            0x87654321_abcdef01
        );
        mem.write_int(teb + 0x1480 + slot * 8, 8, 0xfedcba98_12345678)
            .unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "TlsGetValue",
                &[0xfacefeed_00000000 | slot],
            ),
            0xfedcba98_12345678
        );
        assert_eq!(mem.read_int(teb + 0x68, 4).unwrap(), 0);
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, "TlsFree", &[slot]),
            1
        );
        assert_eq!(mem.read_int(teb + 0x1480 + slot * 8, 8).unwrap(), 0);
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "TlsGetValue",
                &[slot]
            ),
            0
        );
        assert_eq!(mem.read_int(teb + 0x68, 4).unwrap(), 87);
    }
    for name in [
        "TlsAlloc",
        "TlsGetValue",
        "TlsSetValue",
        "TlsFree",
        "FlsAlloc",
        "FlsGetValue",
        "FlsSetValue",
        "FlsFree",
    ] {
        assert!(!wire::x64::runtime::import_supported("user32.dll", name));
    }
}

#[test]
fn locals_keep_win64_allocation_preflight_and_slot_reuse() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let page = wire::x64::runtime::TEB + 4096;
    mem.virtual_protect(page, 4096, wire::mem::PAGE_READONLY)
        .unwrap();
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "TlsAlloc", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, 0);
    mem.virtual_protect(page, 4096, wire::mem::PAGE_READWRITE)
        .unwrap();
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "TlsAlloc", &[]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "TlsFree", &[0]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "TlsAlloc", &[]),
        0
    );
}

#[test]
fn locals_keep_win64_fls_callback_rejection_before_retiring_slot() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let slot = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "FlsAlloc",
        &[0xfacefeed_00000000],
    );
    assert_eq!(slot, 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "FlsSetValue",
            &[slot, 0x87654321_abcdef01]
        ),
        1
    );
    mem.write_int(wire::x64::runtime::TEB + 0x68, 4, 0x55)
        .unwrap();
    let before = cpu.clone();
    let calls = runtime.api_calls;
    let error = runtime
        .dispatch("kernel32.dll", "FlsFree", &mut cpu, &mut mem)
        .unwrap_err();
    assert_eq!(error, "Win64 FlsFree callbacks not implemented");
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(
        mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(),
        0x55
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "FlsGetValue", &[slot]),
        0x87654321_abcdef01
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "FlsSetValue", &[slot, 0]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "FlsFree", &[slot]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "FlsGetValue", &[slot]),
        0
    );
    assert_eq!(mem.read_int(wire::x64::runtime::TEB + 0x68, 4).unwrap(), 87);
}

#[test]
fn locals_keep_fresh_provider_lookup_and_multimedia_clock_scope() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let module_name = STACK + 0x100;
    let symbol = STACK + 0x200;
    for dll in ["kernel32.dll", "kernelbase.dll", "winmm.dll", "user32.dll"] {
        runtime.register_provider(dll);
        let wide: Vec<u8> = dll
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        mem.write(module_name, &wide).unwrap();
        let module = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetModuleHandleW",
            &[module_name],
        );
        assert_ne!(module, 0);
        for name in [
            "TlsAlloc",
            "TlsGetValue",
            "TlsSetValue",
            "TlsFree",
            "FlsAlloc",
            "FlsGetValue",
            "FlsSetValue",
            "FlsFree",
            "timeGetTime",
        ] {
            mem.write(symbol, format!("{name}\0").as_bytes()).unwrap();
            let address = startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetProcAddress",
                &[module, symbol],
            );
            let supported = if name == "timeGetTime" {
                dll == "winmm.dll"
            } else {
                matches!(dll, "kernel32.dll" | "kernelbase.dll")
            };
            assert_eq!(address != 0, supported, "{dll}!{name}");
        }
    }
}
