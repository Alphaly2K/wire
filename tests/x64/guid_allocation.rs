//! Independent provider, pointer-width and saved-frame GUID baseline.
use super::*;

#[test]
fn guid_allocation_keeps_scoped_imports_and_input_and_return_slot_aliases() {
    let names = ["CoCreateGuid", "StringFromCLSID", "StringFromIID"];
    for dll in ["ole32.dll", "combase.dll"] {
        for name in names {
            assert!(wire::x64::runtime::import_supported(dll, name));
        }
    }
    for dll in [
        "kernel32.dll",
        "kernelbase.dll",
        "oleaut32.dll",
        "user32.dll",
    ] {
        for name in names {
            assert!(!wire::x64::runtime::import_supported(dll, name));
        }
    }
    let (mut mem, mut cpu, mut rt) = startup();
    let input = STACK + 0x100;
    let guid = [
        0x33, 0x22, 0x11, 0, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
    ];
    let expected: Vec<_> = "{00112233-4455-6677-8899-AABBCCDDEEFF}\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    for (name, output) in [
        ("StringFromCLSID", input),
        ("StringFromIID", STACK + 0x1f88),
    ] {
        mem.write(input, &guid).unwrap();
        startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "combase.dll",
                name,
                &[input, output]
            ),
            0
        );
        let pointer = mem.read_int(output, 8).unwrap();
        assert!(pointer > u32::MAX as u64);
        let mut actual = [0; 78];
        mem.read(pointer, &mut actual).unwrap();
        assert_eq!(actual.as_slice(), expected);
        assert_eq!(cpu.rip, BASE);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            123
        );
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "ole32.dll",
            "CoTaskMemFree",
            &[pointer],
        );
        assert!(mem.read_int(pointer, 1).is_err());
    }
}

#[test]
fn guid_allocation_keeps_full_pointer_faults_output_and_saved_frame_atomicity() {
    let (mut mem, mut cpu, mut rt) = startup();
    let low = 0x20000;
    mem.map(low, 4096, READ | WRITE).unwrap();
    mem.write(low, &[0x55; 16]).unwrap();
    let output = STACK + 0x100;
    for (name, args) in [
        ("CoCreateGuid", vec![low + (1 << 32)]),
        ("StringFromCLSID", vec![low + (1 << 32), output]),
        ("StringFromIID", vec![low, low + (1 << 32)]),
    ] {
        cpu.regs[4] = STACK + 0x1f88;
        mem.write_int(cpu.regs[4], 8, BASE).unwrap();
        mem.write(output, &[0x55; 8]).unwrap();
        for (i, value) in args.into_iter().enumerate() {
            cpu.regs[[1, 2][i]] = value;
        }
        let before = cpu.clone();
        let calls = rt.api_calls;
        let memory = mem.memory_status();
        assert!(rt.dispatch("ole32.dll", name, &mut cpu, &mut mem).is_err());
        assert_eq!(cpu.regs, before.regs);
        assert_eq!(cpu.rip, before.rip);
        assert_eq!(rt.api_calls, calls);
        assert_eq!(mem.memory_status(), memory);
        assert_eq!(mem.read_int(output, 8).unwrap(), 0x5555555555555555);
        assert_eq!(mem.read_int(low, 8).unwrap(), 0x5555555555555555);
    }
}
