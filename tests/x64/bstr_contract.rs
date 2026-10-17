//! Independent BSTR membership, ordinal, carrier and return-frame baseline.
use super::*;

#[test]
fn bstr_contract_keeps_win64_provider_ordinals_and_unowned_length_prefixes() {
    let (mut mem, mut cpu, mut rt) = startup();
    let source = STACK + 0x100;
    let exports = [
        (2, "SysAllocString"),
        (3, "SysReAllocString"),
        (4, "SysAllocStringLen"),
        (5, "SysReAllocStringLen"),
        (6, "SysFreeString"),
        (7, "SysStringLen"),
        (149, "SysStringByteLen"),
        (150, "SysAllocStringByteLen"),
    ];
    mem.write(source, b"oleaut32.dll\0").unwrap();
    let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[source]);
    for (ordinal, name) in exports {
        assert!(wire::x64::runtime::import_supported(
            "OLEAUT32.dll",
            &format!("#{ordinal}")
        ));
        for dll in [
            "kernel32.dll",
            "kernelbase.dll",
            "ole32.dll",
            "combase.dll",
            "user32.dll",
        ] {
            assert!(
                !wire::x64::runtime::import_supported(dll, name),
                "{dll}!{name}"
            );
            assert!(!wire::x64::runtime::import_supported(
                dll,
                &format!("#{ordinal}")
            ));
        }
        let gateway = startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetProcAddress",
            &[module, ordinal],
        );
        assert_ne!(gateway, 0);
        assert_eq!(rt.gateways[&gateway], format!("oleaut32.dll!{name}"));
    }
    mem.write_int(source, 4, 5).unwrap();
    for (name, expected) in [("SysStringLen", 2), ("SysStringByteLen", 5)] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "oleaut32.dll",
                name,
                &[source + 4]
            ),
            expected
        );
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut rt, "oleaut32.dll", name, &[0]),
            0
        );
    }
}

#[test]
fn bstr_contract_keeps_win64_byte_widths_membership_and_saved_return_alias() {
    let (mut mem, mut cpu, mut rt) = startup();
    let source = STACK + 0x100;
    let output = source + 0x100;
    mem.write(source, b"abc\0").unwrap();
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
    let old = startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "oleaut32.dll",
        "SysAllocStringByteLen",
        &[source, 0xdeadbeef_00000003],
    );
    assert!(old > u32::MAX as u64);
    assert_eq!(mem.read_int(old - 4, 4).unwrap(), 3);
    assert_eq!(mem.read_int(old + 3, 2).unwrap(), 0);
    let slot = STACK + 0x1f88;
    mem.write_int(slot, 8, old).unwrap();
    cpu.regs[4] = slot;
    cpu.regs[1] = slot;
    cpu.regs[2] = source;
    cpu.regs[8] = 0xdeadbeef_00000002;
    assert_eq!(
        rt.dispatch("oleaut32.dll", "SysReAllocStringLen", &mut cpu, &mut mem)
            .unwrap(),
        wire::x64::runtime::Control::Returned
    );
    let next = mem.read_int(slot, 8).unwrap();
    assert_ne!(next, old);
    assert_eq!(cpu.rip, old);
    assert_eq!(cpu.regs[4], slot + 8);
    assert_eq!(cpu.regs[0], 1);
    assert!(mem.read_int(old, 1).is_err());
    assert_eq!(mem.read_int(next - 4, 4).unwrap(), 4);
    mem.write_int(output, 8, next).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "oleaut32.dll",
            "SysReAllocStringLen",
            &[output, source, 0x80000000]
        ),
        0
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), next);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        123
    );
    startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "oleaut32.dll",
        "SysFreeString",
        &[next],
    );
    cpu.regs[4] = slot;
    mem.write_int(slot, 8, BASE).unwrap();
    cpu.regs[1] = next;
    let before = cpu.clone();
    let calls = rt.api_calls;
    assert!(rt
        .dispatch("oleaut32.dll", "SysFreeString", &mut cpu, &mut mem)
        .unwrap_err()
        .contains("invalid or released BSTR allocation"));
    assert_eq!(cpu, before);
    assert_eq!(rt.api_calls, calls);
}

#[test]
fn bstr_contract_keeps_win64_output_fault_and_overlap_before_reallocation() {
    let (mut mem, mut cpu, mut rt) = startup();
    let output = STACK + 0x100;
    let old = startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "oleaut32.dll",
        "SysAllocStringLen",
        &[0, 8],
    );
    mem.write_int(output, 8, old).unwrap();
    mem.write_int(old, 8, old).unwrap();
    let memory = mem.memory_status();
    for target in [old, output & 0xffff_ffff] {
        cpu.regs[4] = STACK + 0x1f88;
        mem.write_int(cpu.regs[4], 8, BASE).unwrap();
        cpu.regs[1] = target;
        cpu.regs[2] = 0;
        cpu.regs[8] = 4;
        let before = cpu.clone();
        let calls = rt.api_calls;
        assert!(rt
            .dispatch("oleaut32.dll", "SysReAllocStringLen", &mut cpu, &mut mem)
            .is_err());
        assert_eq!(cpu, before);
        assert_eq!(rt.api_calls, calls);
        assert_eq!(mem.memory_status(), memory);
        assert_eq!(mem.read_int(output, 8).unwrap(), old);
    }
    startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "oleaut32.dll",
        "SysFreeString",
        &[old],
    );
}
