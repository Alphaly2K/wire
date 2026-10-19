//! Independent legacy class ABI, full-width identity and validation order.
use super::*;

#[test]
fn common_controls_contract_win64_keeps_initialization_exports_absent() {
    for dll in [
        "comctl32.dll",
        "kernel32.dll",
        "kernelbase.dll",
        "user32.dll",
    ] {
        for name in ["InitCommonControls", "InitCommonControlsEx"] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
        }
    }
}

#[test]
fn classes_contract_win64_keeps_static_aliases_dynamic_scope_and_absent_queries() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll", "gdi32.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "RegisterClassA",
            "RegisterClassW",
            "RegisterClassExA",
            "RegisterClassExW",
            "UnregisterClassA",
            "UnregisterClassW",
        ] {
            assert_eq!(
                wire::x64::runtime::import_supported(dll, name),
                dll != "gdi32.dll"
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
                dll == "user32.dll"
            );
        }
        for name in [
            "GetClassInfoA",
            "GetClassInfoW",
            "GetClassInfoExA",
            "GetClassInfoExW",
            "GetClassLongA",
            "GetClassLongW",
            "SetClassLongA",
            "SetClassLongW",
            "GetClassWord",
            "GetClassNameA",
            "GetClassNameW",
        ] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
        }
    }
}

#[test]
fn classes_contract_win64_keeps_full_instance_duplicate_and_menu_rules() {
    let (mut mem, mut cpu, mut rt) = startup();
    let record = STACK + 0x200;
    let name = STACK + 0x300;
    mem.write(name, b"WireClass\0").unwrap();
    for (offset, width, value) in [
        (0, 4, 80),
        (8, 8, BASE),
        (20, 4, 8),
        (24, 8, BASE),
        (64, 8, name),
    ] {
        mem.write_int(record + offset, width, value).unwrap();
    }
    let call =
        |mem: &mut Memory,
         cpu: &mut Cpu64,
         rt: &mut wire::x64::runtime::Runtime64,
         name,
         args: &[u64]| startup_dll_call(mem, cpu, rt, "user32.dll", name, args);
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[77]);
    let atom = call(&mut mem, &mut cpu, &mut rt, "RegisterClassExA", &[record]);
    assert_eq!(atom, 0xc000);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        77
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "RegisterClassExA", &[record]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        1410
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "UnregisterClassA",
            &[atom, BASE as u32 as u64]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        1411
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "UnregisterClassA",
            &[atom, BASE]
        ),
        1
    );
    mem.write_int(record + 56, 8, 1).unwrap();
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "RegisterClassExA", &[record]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        50
    );
    mem.write_int(record + 56, 8, 0).unwrap();
    mem.write_int(record + 16, 4, 1).unwrap();
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "RegisterClassExA", &[record]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        50
    );
    mem.write_int(record + 16, 4, 0).unwrap();
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "RegisterClassExA", &[record]),
        0xc001
    );
}

#[test]
fn classes_contract_win64_keeps_record_fault_and_name_byte_limit() {
    let (mut mem, mut cpu, mut rt) = startup();
    cpu.regs[1] = STACK + 8192 - 72;
    let before = (cpu.rip, cpu.regs[4], rt.api_calls);
    assert!(rt
        .dispatch("user32.dll", "RegisterClassExW", &mut cpu, &mut mem)
        .is_err());
    assert_eq!((cpu.rip, cpu.regs[4], rt.api_calls), before);
    let record = STACK + 0x200;
    let name = STACK + 0x300;
    let wide: Vec<u8> = "é"
        .repeat(129)
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    mem.write(name, &wide).unwrap();
    for (offset, width, value) in [(0, 4, 80), (8, 8, BASE), (24, 8, BASE), (64, 8, name)] {
        mem.write_int(record + offset, width, value).unwrap();
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "RegisterClassExW",
            &[record]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    mem.write(name, &[b'A', 0, 0, 0]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "RegisterClassExW",
            &[record]
        ),
        0xc000
    );
}
