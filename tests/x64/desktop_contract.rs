//! Independent desktop provider scope, process-local settings and output order.
use super::*;

#[test]
fn desktop_contract_win64_keeps_static_aliases_and_settings_lookup_gap() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll", "dwmapi.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in ["SystemParametersInfoA", "SystemParametersInfoW"] {
            assert_eq!(
                wire::x64::runtime::import_supported(dll, name),
                dll != "dwmapi.dll"
            );
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text]
                ),
                0
            );
            if dll != "dwmapi.dll" {
                let gateway = rt.register(dll, name);
                assert_eq!(
                    startup_call(
                        &mut mem,
                        &mut cpu,
                        &mut rt,
                        "GetProcAddress",
                        &[module, text]
                    ),
                    gateway
                );
            }
        }
        for name in ["DwmGetWindowAttribute", "DwmIsCompositionEnabled"] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text]
                ) != 0,
                matches!(dll, "user32.dll" | "dwmapi.dll")
            );
        }
        for name in ["GetDoubleClickTime", "GetDialogBaseUnits"] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
        }
    }
}

#[test]
fn desktop_contract_win64_keeps_screen_reader_state_truncation_and_dwm_errors() {
    let (mut mem, mut cpu, mut rt) = startup();
    let output = STACK + 0x100;
    let call =
        |mem: &mut Memory,
         cpu: &mut Cpu64,
         rt: &mut wire::x64::runtime::Runtime64,
         name: &str,
         args: &[u64]| startup_dll_call(mem, cpu, rt, "user32.dll", name, args);
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[77]);
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SystemParametersInfoA",
            &[0xabcdef01_00000046, 99, output, u64::MAX]
        ),
        1
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        77
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SystemParametersInfoW",
            &[0x47, 0xabcdef01_00000000, 1, 0xabcdef01_00000000]
        ),
        1
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SystemParametersInfoA",
            &[0x47, 1, 1, 1]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        50
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SystemParametersInfoW",
            &[0x46, 0, output, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 0);
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SystemParametersInfoA",
            &[0x47, 1, 1, 0]
        ),
        1
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SystemParametersInfoW",
            &[0x46, 0, output, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 1);
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SystemParametersInfoA",
            &[u32::MAX as u64, 0, output, 0]
        ),
        0
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 1);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "dwmapi.dll",
            "DwmGetWindowAttribute",
            &[1u64 << 32, 9, 0, 16]
        ),
        0x80070006
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "dwmapi.dll",
            "DwmIsCompositionEnabled",
            &[0]
        ),
        0x80070057
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "dwmapi.dll",
            "DwmIsCompositionEnabled",
            &[output]
        ),
        0
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        50
    );
}

#[test]
fn desktop_contract_win64_keeps_fault_frame_and_output_alias_order() {
    let (mut mem, mut cpu, mut rt) = startup();
    let output = STACK + 4096 - 2;
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    cpu.regs[1] = 0x46;
    cpu.regs[2] = 0;
    cpu.regs[8] = output;
    cpu.regs[9] = 0;
    let before = (cpu.rip, cpu.regs[4], rt.api_calls);
    assert!(rt
        .dispatch("user32.dll", "SystemParametersInfoA", &mut cpu, &mut mem)
        .is_err());
    assert_eq!((cpu.rip, cpu.regs[4], rt.api_calls), before);
    mem.protect(STACK + 4096, 4096, READ | WRITE).unwrap();
    let return_slot = STACK + 0x1f88;
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "dwmapi.dll",
            "DwmIsCompositionEnabled",
            &[return_slot]
        ),
        0
    );
    assert_eq!(mem.read_int(return_slot, 4).unwrap(), 0);
}
