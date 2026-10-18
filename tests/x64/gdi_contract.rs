//! Independent Win64 export scope, full-width DC identity and checked frames.
use super::*;

#[test]
fn gdi_contract_win64_keeps_static_aliases_and_dynamic_provider_scope() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll", "gdi32.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "GetDC",
            "ReleaseDC",
            "CreateCompatibleDC",
            "DeleteDC",
            "GetDeviceCaps",
            "GetPixelFormat",
        ] {
            let user = matches!(name, "GetDC" | "ReleaseDC");
            assert_eq!(
                wire::x64::runtime::import_supported(dll, name),
                dll != "gdi32.dll" || !user,
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
                dll == "user32.dll" || (dll == "gdi32.dll" && !user),
                "{dll}!{name}"
            );
        }
        for name in [
            "GetWindowDC",
            "GdiFlush",
            "CreateDIBSection",
            "SetPixel",
            "GetObjectW",
            "CreateFontW",
        ] {
            assert!(
                !wire::x64::runtime::import_supported(dll, name),
                "{dll}!{name}"
            );
        }
    }
}

#[test]
fn gdi_contract_win64_keeps_profile_dword_truncation_and_dc_ownership() {
    let (mut mem, mut cpu, mut rt) = startup();
    let call = |mem: &mut Memory,
                cpu: &mut Cpu64,
                rt: &mut wire::x64::runtime::Runtime64,
                name: &str,
                args: &[u64]| {
        startup_dll_call(
            mem,
            cpu,
            rt,
            if matches!(name, "GetDC" | "ReleaseDC") {
                "user32.dll"
            } else {
                "gdi32.dll"
            },
            name,
            args,
        )
    };
    let screen = call(&mut mem, &mut cpu, &mut rt, "GetDC", &[0]);
    assert!(screen > u32::MAX as u64);
    let compatible = call(&mut mem, &mut cpu, &mut rt, "CreateCompatibleDC", &[screen]);
    assert!(compatible > screen);
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[77]);
    for (index, expected) in [
        (0, 0x400),
        (8, 1024),
        (10, 768),
        (24, u32::MAX as u64),
        (38, 0),
        (40, 36),
        (88, 96),
        (116, 60),
        (u32::MAX, 0),
    ] {
        assert_eq!(
            call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "GetDeviceCaps",
                &[screen, 0xabcdef01_00000000 | index as u64]
            ),
            expected
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            77
        );
    }
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "GetPixelFormat", &[screen]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        77
    );
    for (name, args) in [
        ("DeleteDC", vec![screen]),
        ("ReleaseDC", vec![0, compatible]),
        ("GetDeviceCaps", vec![screen as u32 as u64, 88]),
    ] {
        assert_eq!(call(&mut mem, &mut cpu, &mut rt, name, &args), 0);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            6
        );
    }
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "GetDeviceCaps", &[screen, 88]),
        96
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "ReleaseDC", &[0, screen]),
        1
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "GetDeviceCaps", &[screen, 88]),
        0
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "DeleteDC", &[compatible]),
        1
    );
    assert_eq!(call(&mut mem, &mut cpu, &mut rt, "GetDC", &[1u64 << 32]), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        1400
    );
}

#[test]
fn gdi_contract_win64_bad_return_frame_does_not_acquire_a_dc() {
    let (mut mem, mut cpu, mut rt) = startup();
    cpu.regs[1] = 0;
    cpu.regs[4] = 1;
    let before = cpu.clone();
    let calls = rt.api_calls;
    assert!(rt
        .dispatch("user32.dll", "GetDC", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(rt.api_calls, calls);
    assert_eq!(
        startup_dll_call(&mut mem, &mut cpu, &mut rt, "user32.dll", "GetDC", &[0]),
        0x7ffd00650000
    );
}
