//! Historical providers, virtual cursor clipping, width and output ordering.
use super::*;

#[test]
fn cursor_contract_win64_keeps_static_lookup_and_absent_position_setter() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "ShowCursor",
            "GetCursor",
            "SetCursor",
            "GetCursorPos",
            "ClipCursor",
            "GetClipCursor",
            "EnableMouseInPointer",
            "IsMouseInPointerEnabled",
            "TrackMouseEvent",
            "GetPointerType",
            "GetPointerInfo",
            "GetPointerDevices",
        ] {
            assert_eq!(
                wire::x64::runtime::import_supported(dll, name),
                !matches!(
                    name,
                    "TrackMouseEvent" | "GetPointerType" | "GetPointerInfo" | "GetPointerDevices"
                ) || dll == "user32.dll",
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
                dll == "user32.dll",
                "{dll}!{name}"
            );
        }
        assert!(!wire::x64::runtime::import_supported(dll, "SetCursorPos"));
    }
}

#[test]
fn cursor_contract_win64_keeps_virtual_clip_dword_bool_and_saved_frame() {
    let (mut mem, mut cpu, mut rt) = startup();
    let rect = STACK + 0x100;
    let out = rect + 32;
    let call =
        |mem: &mut Memory,
         cpu: &mut Cpu64,
         rt: &mut wire::x64::runtime::Runtime64,
         name: &str,
         args: &[u64]| startup_dll_call(mem, cpu, rt, "user32.dll", name, args);
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[77]);
    mem.write(
        rect,
        &[10i32, 20, 30, 40]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(call(&mut mem, &mut cpu, &mut rt, "ClipCursor", &[rect]), 1);
    assert_eq!(call(&mut mem, &mut cpu, &mut rt, "GetCursorPos", &[out]), 1);
    assert_eq!(mem.read_int(out, 8).unwrap(), (20u64 << 32) | 10);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        77
    );
    mem.write_int(rect + 8, 4, 9).unwrap();
    assert_eq!(call(&mut mem, &mut cpu, &mut rt, "ClipCursor", &[rect]), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "GetClipCursor", &[out]),
        1
    );
    assert_eq!(mem.read_int(out + 8, 8).unwrap(), (40u64 << 32) | 30);
    assert_eq!(call(&mut mem, &mut cpu, &mut rt, "ClipCursor", &[0]), 1);
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "GetClipCursor", &[out]),
        1
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0);
    assert_eq!(mem.read_int(out + 8, 8).unwrap(), (768u64 << 32) | 1024);
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[77]);
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "EnableMouseInPointer",
            &[1u64 << 32]
        ),
        1
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "IsMouseInPointerEnabled", &[]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        77
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "SetCursor", &[1u64 << 32]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        6
    );
    assert_eq!(call(&mut mem, &mut cpu, &mut rt, "GetCursor", &[]), 0);
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "ShowCursor", &[1u64 << 32]),
        u32::MAX as u64
    );
    assert_eq!(call(&mut mem, &mut cpu, &mut rt, "ShowCursor", &[1]), 0);
    let slot = STACK + 0x1f88;
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "GetCursorPos", &[slot]),
        1
    );
    assert_eq!(mem.read_int(slot, 8).unwrap(), (20u64 << 32) | 10);
    assert_eq!(cpu.rip, BASE);
}

#[test]
fn cursor_contract_win64_output_fault_preserves_cpu_and_accessible_prefix() {
    let (mut mem, mut cpu, mut rt) = startup();
    let output = STACK + 4096 - 4;
    mem.write(output, &[0xa5; 16]).unwrap();
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    for name in ["GetCursorPos", "GetClipCursor"] {
        cpu.regs[1] = output;
        let before = (cpu.rip, cpu.regs[4], rt.api_calls);
        assert!(rt.dispatch("user32.dll", name, &mut cpu, &mut mem).is_err());
        assert_eq!((cpu.rip, cpu.regs[4], rt.api_calls), before);
        assert_eq!(mem.read_int(output, 4).unwrap(), 0xa5a5a5a5);
    }
}

#[test]
fn cursor_contract_win64_pointer_digitizer_list_is_empty_with_checked_count() {
    let (mut mem, mut cpu, mut rt) = startup();
    let count = STACK + 0x100;
    let output = count + 16;
    mem.write_int(count, 4, 0x1234).unwrap();
    mem.write(output, &[0xaa; 32]).unwrap();
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[77]);
    for data in [0, output] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "user32.dll",
                "GetPointerDevices",
                &[count, data]
            ),
            1
        );
        assert_eq!(mem.read_int(count, 4).unwrap(), 0);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            77
        );
    }
    let mut bytes = [0; 32];
    mem.read(output, &mut bytes).unwrap();
    assert_eq!(bytes, [0xaa; 32]);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "user32.dll",
            "GetPointerDevices",
            &[0, output]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    let bad = STACK + 8190;
    mem.write(bad, &[0xbb; 2]).unwrap();
    cpu.regs[1] = bad;
    cpu.regs[2] = output;
    let before = cpu.clone();
    assert!(rt
        .dispatch("user32.dll", "GetPointerDevices", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(cpu.regs, before.regs);
    let mut prefix = [0; 2];
    mem.read(bad, &mut prefix).unwrap();
    assert_eq!(prefix, [0xbb; 2]);
}
