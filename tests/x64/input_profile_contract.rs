//! Static/lookup providers and fault ordering before input-profile migration.
use super::*;

#[test]
fn input_profile_contract_win64_preserves_static_and_dynamic_provider_sets() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll", "imm32.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        assert_ne!(module, 0);
        for (name, ime) in [("GetRawInputDeviceList", false), ("ImmIsIME", true)] {
            assert_eq!(
                wire::x64::runtime::import_supported(dll, name),
                ime || dll != "imm32.dll",
                "{dll}!{name}"
            );
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            let address = startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "GetProcAddress",
                &[module, text],
            );
            assert_eq!(
                address != 0,
                dll == "user32.dll" || (ime && dll == "imm32.dll"),
                "{dll}!{name}"
            );
        }
    }
}

#[test]
fn input_profile_contract_win64_keeps_raw_count_alias_and_validation_precedence() {
    let (mut mem, mut cpu, mut rt) = startup();
    let slot = STACK + 0x1f88;
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "GetRawInputDeviceList",
            &[0, slot, 0xabcdef01_00000010]
        ),
        0
    );
    assert_eq!(mem.read_int(slot, 8).unwrap(), 0x1_00000002);
    assert_eq!(cpu.rip, BASE);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        123
    );
    for (name, args, error) in [
        ("RegisterRawInputDevices", vec![1, 1, 15], 87),
        ("GetRawInputDeviceList", vec![1, 1, 15], 87),
        ("GetRawInputBuffer", vec![1, 1, 16], 87),
        ("GetRawInputData", vec![1, 0x10000003, 1, 1, 16], 87),
        ("GetRawInputData", vec![1, 0x10000003, 1, 1, 24], 6),
        ("GetRawInputDeviceInfoW", vec![1, 0, 1, 1], 6),
    ] {
        let expected = if name == "RegisterRawInputDevices" {
            0
        } else {
            u32::MAX as u64
        };
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut rt, "user32.dll", name, &args),
            expected,
            "{name}"
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            error,
            "{name}"
        );
    }
}

#[test]
fn input_profile_contract_win64_removal_keeps_dword_counts_and_full_target_width() {
    let (mut mem, mut cpu, mut rt) = startup();
    let device = STACK + 0x200;
    mem.write_int(device, 2, 1).unwrap();
    mem.write_int(device + 2, 2, 2).unwrap();
    for (flags, target, expected, error) in [(0, 0, 1, 123), (1, 0, 1, 123), (1, 1u64 << 32, 0, 87)]
    {
        mem.write_int(device + 4, 4, flags).unwrap();
        mem.write_int(device + 8, 8, target).unwrap();
        startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "user32.dll",
                "RegisterRawInputDevices",
                &[device, 0xabcdef01_00000001, 0xabcdef01_00000010]
            ),
            expected
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            error
        );
    }
}

#[test]
fn input_profile_contract_win64_ime_keeps_null_desktop_and_window_error_precedence() {
    let (mut mem, mut cpu, mut rt) = startup();
    for (name, args, error) in [
        ("ImmIsIME", vec![u64::MAX], 123),
        ("ImmGetDefaultIMEWnd", vec![0], 123),
        ("ImmGetDefaultIMEWnd", vec![u64::MAX], 1400),
        ("ImmGetContext", vec![0], 1400),
        ("ImmReleaseContext", vec![0, u64::MAX], 1400),
        ("ImmAssociateContext", vec![0, u64::MAX], 1400),
        ("ImmAssociateContextEx", vec![0, 0, 1], 1400),
        ("ImmGetConversionStatus", vec![0, 1, 1], 6),
    ] {
        startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut rt, "imm32.dll", name, &args),
            0,
            "{name}"
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            error,
            "{name}"
        );
    }
}
