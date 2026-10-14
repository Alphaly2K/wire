//! Independent NLS compatibility baselines captured before adapter migration.
#[allow(dead_code)]
#[path = "support/x64.rs"]
mod x64;
#[allow(dead_code)]
#[path = "support/x86.rs"]
mod x86;

#[test]
fn pe32_nls_keeps_partial_output_error_mapping_and_legacy_encoding_rules() {
    let mut v = x86::Vm::new();
    let input = x86::DATA;
    let output = input + 256;
    let used = output + 256;
    v.wide(input, "A");
    v.mem.write_u32(used, 99).unwrap();
    v.mem.write_bytes(output, &[0xcc; 8]).unwrap();
    assert_eq!(
        v.k(
            "WideCharToMultiByte",
            &[932, 0, input, u32::MAX, output, 1, 0, used]
        ),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 122);
    assert_eq!(v.mem.read_u32(used).unwrap(), 0); // precedes insufficient-buffer check
    assert_eq!(v.mem.read_u8(output).unwrap(), 0xcc);
    v.mem.write_bytes(input, b"A\0").unwrap();
    assert_eq!(
        v.k(
            "MultiByteToWideChar",
            &[932, 0x100, input, u32::MAX, output, 2]
        ),
        2
    );
    assert_eq!(v.mem.read_u32(output).unwrap(), 65); // legacy flags are ignored
    assert_eq!(v.k("MultiByteToWideChar", &[932, 0, 1, 1, output, 2]), 0);
    assert_eq!(v.k("GetLastError", &[]), 87);
    assert!(v.try_call("kernel32.dll", "GetCPInfo", &[932, 1]).is_err());
    assert_eq!(v.k("GetCPInfo", &[3, output]), 0);
    assert_eq!(v.k("GetLastError", &[]), 87); // PE32 does not alias CP_THREAD_ACP
    assert_eq!(v.k("IsDBCSLeadByteEx", &[65001, 0x81]), 1);
    assert_eq!(v.k("GetCPInfoExW", &[932, 0, output]), 0);
    assert_eq!(v.k("GetLastError", &[]), 120); // ABI-only on PE32
    assert_eq!(v.k("GetLocaleInfoW", &[0x411, 1, output, 8]), 0);
    assert_eq!(v.k("GetLastError", &[]), 120);
    assert!(
        wire::nls::dispatch("kernel32.dll", "GetACP", &[0; 16], &mut v.mem, &mut v.state).is_none()
    );
    let result = wire::nls::dispatch(
        "kernel32.dll",
        "MultiByteToWideChar",
        &[
            932,
            0,
            input,
            u32::MAX,
            output,
            2,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ],
        &mut v.mem,
        &mut v.state,
    );
    assert_eq!(result.unwrap().0, 2);
}

#[test]
fn nls_return_slot_alias_and_wrong_provider_keep_architecture_commit_rules() {
    let mut v = x86::Vm::new();
    v.mem.write_bytes(x86::DATA, b"A\0").unwrap();
    let sp = wire::loader::STACK_BASE + wire::loader::STACK_SIZE - 256;
    assert_eq!(
        v.k("MultiByteToWideChar", &[932, 0, x86::DATA, u32::MAX, sp, 2]),
        2
    );
    assert_eq!(v.cpu.eip, 65); // output overwrites return address before it is read
    assert_eq!(v.cpu.esp(), sp + 28);
    assert_eq!(v.call("kernelbase.dll", "GetACP", &[]), 0);
    assert_eq!(v.state.last_error, 120);
    let (mut mem, mut cpu, mut runtime) = x64::startup();
    for name in ["IsDBCSLeadByte", "LCMapStringA", "GetStringTypeExW"] {
        assert!(!wire::x64::runtime::import_supported("kernel32.dll", name));
    }
    let input = x64::STACK + 0x100;
    mem.write(input, b"A\0").unwrap();
    let sp = x64::STACK + 0x1f88;
    assert_eq!(
        x64::startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MultiByteToWideChar",
            &[932, 0, input, u32::MAX as u64, sp, 2]
        ),
        2
    );
    assert_eq!(cpu.rip, x64::BASE); // Win64 saves frame before output
}

#[test]
fn win64_nls_keeps_strict_flags_preflight_and_provider_scopes() {
    let (mut mem, mut cpu, mut runtime) = x64::startup();
    let input = x64::STACK + 0x100;
    let output = input + 256;
    let used = output + 256;
    mem.write(input, b"A\0").unwrap();
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        assert_eq!(
            x64::startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, "GetACP", &[]),
            932
        );
    }
    mem.write_int(output, 8, 0xcccccccccccccccc).unwrap();
    assert_eq!(
        x64::startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MultiByteToWideChar",
            &[932, 0x100, input, u32::MAX as u64, output, 2]
        ),
        0
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 0xcccccccccccccccc);
    assert_eq!(
        x64::startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        1004
    );
    assert_eq!(
        x64::startup_call(&mut mem, &mut cpu, &mut runtime, "GetCPInfo", &[3, output]),
        1
    );
    mem.write(input, &[65, 0, 0, 0]).unwrap();
    mem.write_int(used, 4, 99).unwrap();
    assert_eq!(
        x64::startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WideCharToMultiByte",
            &[932, 0, input, u32::MAX as u64, output, 1, 0, used]
        ),
        0
    );
    assert_eq!(mem.read_int(used, 4).unwrap(), 99); // size check precedes used-char output
    let before = cpu.clone();
    assert!(runtime
        .dispatch("user32.dll", "GetACP", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
}
