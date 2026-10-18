//! Image identity, ownership, parameter widths and historical provider scope.
use super::*;

#[test]
fn images_contract_win64_keeps_static_kernel_and_dynamic_user_providers() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "LoadIconA",
            "LoadIconW",
            "LoadCursorA",
            "LoadCursorW",
            "LoadImageA",
            "LoadImageW",
            "DestroyIcon",
            "DestroyCursor",
        ] {
            assert!(
                wire::x64::runtime::import_supported(dll, name),
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
    }
}

#[test]
fn images_contract_win64_keeps_shared_identity_owned_lifetime_and_full_handles() {
    let (mut mem, mut cpu, mut rt) = startup();
    let call =
        |mem: &mut Memory,
         cpu: &mut Cpu64,
         rt: &mut wire::x64::runtime::Runtime64,
         name: &str,
         args: &[u64]| startup_dll_call(mem, cpu, rt, "user32.dll", name, args);
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[77]);
    let shared = call(&mut mem, &mut cpu, &mut rt, "LoadCursorA", &[0, 32512]);
    assert!(shared > u32::MAX as u64);
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "LoadCursorW", &[0, 32512]),
        shared
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "DestroyCursor", &[shared]),
        1
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "LoadImageW",
            &[0, 32512, 2, 0, 0, 0x8000]
        ),
        shared
    );
    let owned = call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "LoadImageA",
        &[
            0,
            32512,
            0xabcdef01_00000002,
            1u64 << 32,
            1u64 << 32,
            1u64 << 32,
        ],
    );
    assert_ne!(owned, shared);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        77
    );
    assert_eq!(call(&mut mem, &mut cpu, &mut rt, "SetCursor", &[owned]), 0);
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "DestroyIcon", &[owned]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        6
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "DestroyCursor",
            &[owned as u32 as u64]
        ),
        0
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "DestroyCursor", &[owned]),
        1
    );
    // Destroying an owned resource does not rewrite the selected logical handle.
    assert_eq!(call(&mut mem, &mut cpu, &mut rt, "GetCursor", &[]), owned);
    assert_eq!(call(&mut mem, &mut cpu, &mut rt, "SetCursor", &[owned]), 0);
    assert_eq!(call(&mut mem, &mut cpu, &mut rt, "GetCursor", &[]), owned);
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "DestroyCursor", &[owned]),
        0
    );
}

#[test]
fn images_contract_win64_keeps_validation_order_and_guest_fault_frame() {
    let (mut mem, mut cpu, mut rt) = startup();
    let call =
        |mem: &mut Memory,
         cpu: &mut Cpu64,
         rt: &mut wire::x64::runtime::Runtime64,
         name: &str,
         args: &[u64]| startup_dll_call(mem, cpu, rt, "user32.dll", name, args);
    // Unsupported dimensions precede reading even an invalid name pointer.
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "LoadImageA",
            &[0, u64::MAX, 2, 1, 0, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        50
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "LoadIconA", &[0, 32519]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        1814
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "LoadIconA",
            &[1u64 << 32, 32512]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        6
    );
    cpu.regs[1] = 0;
    cpu.regs[2] = u64::MAX;
    let before = (cpu.rip, cpu.regs[4], rt.api_calls);
    assert!(rt
        .dispatch("user32.dll", "LoadCursorW", &mut cpu, &mut mem)
        .is_err());
    assert_eq!((cpu.rip, cpu.regs[4], rt.api_calls), before);
}

#[test]
fn images_contract_win64_corrupt_icon_resources_do_not_allocate_or_commit_frames() {
    use wire::x64::process::Process64;
    let path =
        std::env::temp_dir().join(format!("wire-images-contract-{}.exe", std::process::id()));
    let mut bytes = fixture();
    put32(&mut bytes, 0x98 + 112 + 16, 0x1100);
    put32(&mut bytes, 0x98 + 112 + 20, 0x200);
    for (directory, entries) in [
        (0, vec![(3, 0x80000030), (14, 0x80000060)]),
        (0x30, vec![(5, 0x80000090)]),
        (0x60, vec![(103, 0x800000c0)]),
        (0x90, vec![(1033, 0xf0)]),
        (0xc0, vec![(1033, 0x100)]),
    ] {
        let at = 0x300 + directory;
        put16(&mut bytes, at + 14, entries.len() as u16);
        for (i, (name, value)) in entries.into_iter().enumerate() {
            put32(&mut bytes, at + 16 + i * 8, name);
            put32(&mut bytes, at + 20 + i * 8, value);
        }
    }
    for (at, value) in [
        (0x3f0, 0x1300),
        (0x3f4, 64),
        (0x400, 0x1340),
        (0x404, 20),
        (0x500, 40),
        (0x504, 2),
        (0x508, 4),
        (0x54e, 64),
    ] {
        put32(&mut bytes, at, value);
    }
    for (at, value) in [
        (0x50c, 1),
        (0x50e, 32),
        (0x542, 1),
        (0x544, 1),
        (0x546, 0x0202),
        (0x54a, 1),
        (0x54c, 32),
        (0x552, 5),
    ] {
        put16(&mut bytes, at, value);
    }
    std::fs::write(&path, bytes).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    for (address, size, invalid, restored, expected) in [
        (BASE + 0x1340, 2, 7, 0, "invalid icon group header"),
        (BASE + 0x1344, 2, 0, 1, "truncated icon group"),
        (BASE + 0x1304, 4, 0, 2, "invalid icon DIB dimensions/planes"),
    ] {
        p.memory.write_int(address, size, invalid).unwrap();
        p.cpu.regs[4] = STACK + 0x1f88;
        p.memory.write_int(p.cpu.regs[4], 8, BASE).unwrap();
        p.cpu.regs[1] = BASE;
        p.cpu.regs[2] = 103;
        let before = (p.cpu.rip, p.cpu.regs[4], p.runtime.api_calls);
        let error = p
            .runtime
            .dispatch("user32.dll", "LoadIconA", &mut p.cpu, &mut p.memory)
            .unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert_eq!((p.cpu.rip, p.cpu.regs[4], p.runtime.api_calls), before);
        p.memory.write_int(address, size, restored).unwrap();
    }
    let icon = startup_dll_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "user32.dll",
        "LoadIconA",
        &[BASE, 103],
    );
    assert_eq!(icon, wire::x64::runtime::TEB + 0x610000);
    std::fs::remove_file(path).unwrap();
}
