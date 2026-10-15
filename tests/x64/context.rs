//! Full-width pseudo handles, direct PEB/TEB state and hot-frame preflight.
use super::*;

#[test]
fn context_keeps_win64_pointer_width_dword_error_and_peb_debug_byte() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let teb = wire::x64::runtime::TEB;
    let peb = wire::x64::runtime::PEB;
    mem.write_int(teb + 0x68, 4, 0x87654321).unwrap();
    mem.write_int(peb + 2, 1, 0x7f).unwrap();
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        for (name, expected) in [
            ("GetCurrentProcessId", 1),
            ("GetCurrentThreadId", 1),
            ("GetCurrentProcess", u64::MAX),
            ("GetCurrentThread", u64::MAX - 1),
            ("IsDebuggerPresent", 0x7f),
            ("GetLastError", 0x87654321),
        ] {
            assert_eq!(
                startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, name, &[]),
                expected
            );
            assert_eq!(mem.read_int(teb + 0x68, 4).unwrap(), 0x87654321);
            assert!(!wire::x64::runtime::import_supported("user32.dll", name));
        }
    }
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetLastError",
            &[0xfacefeed_fedcba98]
        ),
        0
    );
    assert_eq!(mem.read_int(teb + 0x68, 4).unwrap(), 0xfedcba98);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0xfedcba98
    );
}

#[test]
fn context_keeps_win64_bad_return_frame_ahead_of_last_error_write() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let teb = wire::x64::runtime::TEB;
    mem.write_int(teb + 0x68, 4, 0x55).unwrap();
    cpu.regs[4] = STACK + 8192;
    cpu.regs[1] = 0x87654321;
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "SetLastError", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, 0);
    assert_eq!(mem.read_int(teb + 0x68, 4).unwrap(), 0x55);
}

#[test]
fn context_keeps_win64_delay_general_error_write_and_preflight() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let teb = wire::x64::runtime::TEB;
    runtime
        .install_delays(
            &mut mem,
            STACK,
            &[wire::x64::pe::Delay64 {
                dll: "kernel32.dll".into(),
                module_handle_rva: 0x200,
                functions: vec![wire::pe::ImportFunc {
                    iat_rva: 0x208,
                    name: Some("SetLastError".into()),
                    ordinal: None,
                }],
            }],
        )
        .unwrap();
    let gateway = mem.read_int(STACK + 0x208, 8).unwrap();
    mem.write_int(teb + 0x68, 4, 0x55).unwrap();
    cpu.rip = gateway;
    cpu.regs[4] = STACK + 8192;
    cpu.regs[1] = 0xfacefeed_fedcba98;
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "SetLastError", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, 0);
    assert_eq!(mem.read_int(teb + 0x68, 4).unwrap(), 0x55);
    assert_eq!(mem.read_int(STACK + 0x200, 8).unwrap(), 0);
    let sp = STACK + 0x1f88;
    cpu.regs[4] = sp;
    mem.write_int(sp, 8, BASE).unwrap();
    assert_eq!(
        runtime
            .dispatch("kernel32.dll", "SetLastError", &mut cpu, &mut mem)
            .unwrap(),
        wire::x64::runtime::Control::Returned
    );
    assert_eq!(cpu.rip, BASE);
    assert_eq!(cpu.regs[4], sp + 8);
    assert_eq!(cpu.regs[0], 0);
    assert_eq!(mem.read_int(teb + 0x68, 4).unwrap(), 0xfedcba98);
    assert_ne!(mem.read_int(STACK + 0x200, 8).unwrap(), 0);
    assert_eq!(runtime.api_calls, 1);
}
