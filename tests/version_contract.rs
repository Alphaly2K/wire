//! Independent version API baselines, captured before catalog migration.
#[allow(dead_code)]
#[path = "support/x64.rs"]
mod x64;
#[allow(dead_code)]
#[path = "support/x86.rs"]
mod x86;

#[test]
fn pe32_version_keeps_cross_encoding_sizes_last_error_and_provider_scopes() {
    let mut v = x86::Vm::new();
    v.k("SetLastError", &[777]);
    assert_eq!(v.k("GetVersion", &[]), 0x1db10106);
    for (dll, name) in [
        ("kernel32.dll", "GetVersionExA"),
        ("kernel32.dll", "GetVersionExW"),
        ("ntdll.dll", "RtlGetVersion"),
    ] {
        assert!(wire::api::native_export(dll, name));
        for size in [148, 156, 276, 284] {
            v.mem.write_bytes(x86::DATA, &[0xcc; 300]).unwrap();
            v.mem.write_u32(x86::DATA, size).unwrap();
            v.k("SetLastError", &[777]);
            assert_eq!(
                v.call(dll, name, &[x86::DATA]),
                u32::from(dll != "ntdll.dll")
            );
            for (offset, value) in [(0, size), (4, 6), (8, 1), (12, 7601), (16, 2)] {
                assert_eq!(v.mem.read_u32(x86::DATA + offset).unwrap(), value);
            }
            assert_eq!(v.mem.read_u8(x86::DATA + size).unwrap(), 0xcc);
            assert_eq!(v.k("GetLastError", &[]), 777);
        }
        v.mem.write_u32(x86::DATA, 5).unwrap();
        assert_eq!(v.call(dll, name, &[x86::DATA]), 0);
        assert_eq!(v.k("GetLastError", &[]), 87);
    }
    for (dll, name) in [
        ("kernelbase.dll", "GetVersionExW"),
        ("kernel32.dll", "RtlGetVersion"),
        ("ntdll.dll", "GetVersionExW"),
    ] {
        assert!(!wire::api::native_export(dll, name));
        assert_eq!(v.call(dll, name, &[x86::DATA]), 0);
        assert_eq!(v.state.last_error, 120);
    }
}

#[test]
fn pe32_version_return_slot_alias_is_read_after_output_and_legacy_abi_stays_known() {
    let mut v = x86::Vm::new();
    let sp = wire::loader::STACK_BASE + wire::loader::STACK_SIZE - 256;
    v.mem.write_u32(sp - 4, 148).unwrap();
    assert_eq!(v.call("ntdll.dll", "RtlGetVersion", &[sp - 4]), 0);
    assert_eq!(v.cpu.eip, 6); // output's major version overwrote the return address
    assert_eq!(v.cpu.esp(), sp + 8);
    for name in [
        "VerifyVersionInfoA",
        "VerifyVersionInfoW",
        "VerSetConditionMask",
    ] {
        assert!(wire::api::native_export("kernel32.dll", name));
        assert_eq!(v.k(name, &[x86::DATA, 0, 0, 0]), 0);
        assert_eq!(v.cpu.esp(), sp + 20);
        assert_eq!(v.state.last_error, 120);
    }
    assert!(!wire::api::native_export(
        "ntdll.dll",
        "RtlVerifyVersionInfo"
    ));
    assert_eq!(
        v.call("ntdll.dll", "RtlVerifyVersionInfo", &[x86::DATA, 0, 0, 0]),
        0
    );
    assert_eq!(v.cpu.esp(), sp + 4);
}

#[test]
fn win64_version_provider_scope_and_wide_mask_carriers_keep_the_existing_contract() {
    let (mut mem, mut cpu, mut runtime) = x64::startup();
    let output = x64::STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll", "ntdll.dll"] {
        // Legacy Win64 kernel providers also expose the Rtl form.
        assert!(wire::x64::runtime::import_supported(dll, "RtlGetVersion"));
        mem.write_int(output, 4, 284).unwrap();
        assert_eq!(
            x64::startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "RtlGetVersion",
                &[output]
            ),
            0
        );
        assert_eq!(mem.read_int(output + 4, 4).unwrap(), 10);
        assert_eq!(mem.read_int(output + 12, 4).unwrap(), 19045);
        assert_eq!(
            x64::startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "VerSetConditionMask",
                &[0x8000000000000000, 0x100000002, 0x200000003]
            ),
            0x8000000000000018
        );
    }
    for (dll, name) in [
        ("ntdll.dll", "GetVersionExW"),
        ("kernel32.dll", "VerifyVersionInfoA"),
        ("user32.dll", "GetVersion"),
    ] {
        assert!(!wire::x64::runtime::import_supported(dll, name));
        let before = cpu.clone();
        assert!(runtime.dispatch(dll, name, &mut cpu, &mut mem).is_err());
        assert_eq!(cpu, before);
    }
}
