//! Win64 preserves its own record, feature set, provider and frame contracts.
use super::*;

#[test]
fn process_startup_win64_keeps_command_storage_dword_feature_and_provider_scope() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "GetCommandLineA",
            "GetCommandLineW",
            "GetStartupInfoA",
            "GetStartupInfoW",
            "IsProcessorFeaturePresent",
        ] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            assert!(!wire::x64::runtime::import_supported("user32.dll", name));
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            assert_ne!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text]
                ),
                0
            );
        }
        for (name, offset, expected) in [
            ("GetCommandLineA", 4 * 4096, 0x4322),
            ("GetCommandLineW", 5 * 4096, 0x22),
        ] {
            let pointer = startup_dll_call(&mut mem, &mut cpu, &mut rt, dll, name, &[]);
            assert_eq!(pointer, wire::x64::runtime::TEB + offset);
            assert_eq!(mem.read_int(pointer, 2).unwrap(), expected);
        }
        for (code, expected) in [(6, 0), (10, 0), (12, 1), (0xaaaa0000000c, 1), (0x1000c, 0)] {
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    dll,
                    "IsProcessorFeaturePresent",
                    &[code]
                ),
                expected
            );
        }
    }
}

#[test]
fn process_startup_win64_keeps_record_extent_output_fault_and_saved_return_frame() {
    let (mut mem, mut cpu, mut rt) = startup();
    let out = STACK + 0x100;
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[77]);
    for name in ["GetStartupInfoA", "GetStartupInfoW"] {
        mem.write(out, &[0xa5; 112]).unwrap();
        assert_eq!(startup_call(&mut mem, &mut cpu, &mut rt, name, &[out]), 0);
        assert_eq!(mem.read_int(out, 4).unwrap(), 104);
        let mut tail = [0xff; 100];
        mem.read(out + 4, &mut tail).unwrap();
        assert_eq!(tail, [0; 100]);
        assert_eq!(mem.read_int(out + 104, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            77
        );
        let address = STACK + 8192 - 96;
        mem.write(address, &[0xa5; 96]).unwrap();
        cpu.regs[1] = address;
        let before = (cpu.rip, cpu.regs[4], rt.api_calls);
        assert!(rt
            .dispatch("kernel32.dll", name, &mut cpu, &mut mem)
            .is_err());
        assert_eq!((cpu.rip, cpu.regs[4], rt.api_calls), before);
        let mut unchanged = [0; 96];
        mem.read(address, &mut unchanged).unwrap();
        assert_eq!(unchanged, [0xa5; 96]);
    }
    let sp = STACK + 0x1000;
    cpu.regs[4] = sp;
    cpu.regs[1] = sp;
    mem.write_int(sp, 8, BASE).unwrap();
    assert_eq!(
        rt.dispatch("kernel32.dll", "GetStartupInfoW", &mut cpu, &mut mem)
            .unwrap(),
        wire::x64::runtime::Control::Returned
    );
    assert_eq!(cpu.rip, BASE);
    assert_eq!(cpu.regs[4], sp + 8);
    assert_eq!(mem.read_int(sp, 8).unwrap(), 104);
}
