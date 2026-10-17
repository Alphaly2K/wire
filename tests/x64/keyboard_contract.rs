//! Provider, frame and validation ordering before keyboard catalog migration.
use super::*;

#[test]
fn keyboard_contract_win64_keeps_static_state_providers_and_kernel_lookup_rejection() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        assert_ne!(module, 0);
        for name in ["GetKeyState", "GetAsyncKeyState", "GetKeyboardState"] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            let address = startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "GetProcAddress",
                &[module, text],
            );
            if dll == "user32.dll" {
                assert_ne!(address, 0);
            } else {
                assert_eq!(address, 0);
            }
        }
        for name in ["GetKeyState", "GetAsyncKeyState"] {
            startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    dll,
                    name,
                    &[0xabcdef01_00000041]
                ),
                0
            );
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
                123
            );
        }
    }
    mem.map(STACK + 8192, 4096, READ | WRITE).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "GetKeyboardState",
            &[STACK + 0x1f88]
        ),
        1
    );
    assert_eq!(mem.read_int(STACK + 0x1f88, 8).unwrap(), 0);
    assert_eq!(cpu.rip, BASE);
}

#[test]
fn keyboard_contract_win64_unicode_keeps_validation_precedence_and_state_output_alias() {
    let (mut mem, mut cpu, mut rt) = startup();
    let state = STACK + 0x200;
    for capacity in [0, u32::MAX as u64] {
        startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "user32.dll",
                "ToUnicodeEx",
                &[65, 0, 1, 1, capacity, u32::MAX as u64, u64::MAX]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            123
        );
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "ToUnicodeEx",
            &[65, 0, 1, 1, 1, 0, 0x1_04090409]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    mem.write(state, &[0; 256]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "ToUnicodeEx",
            &[65, 0, state, state, 2, 4, 0x04090409]
        ),
        1
    );
    assert_eq!(mem.read_int(state, 4).unwrap(), 97);
    assert_eq!(mem.read_int(state + 4, 8).unwrap(), 0);
}
