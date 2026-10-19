//! Existing behavioral tests grouped by modules; expectations remain independent of API catalogs.

use super::*;

#[test]
fn win64_keeps_pe32_user_and_mpr_endpoints_unavailable() {
    for (dll, names) in [
        (
            "user32.dll",
            &[
                "GetProcessWindowStation",
                "GetUserObjectInformationW",
                "MessageBoxA",
                "MessageBoxW",
            ][..],
        ),
        (
            "mpr.dll",
            &["WNetGetUniversalNameA", "WNetGetUniversalNameW"][..],
        ),
    ] {
        for name in names {
            assert!(
                !wire::x64::runtime::import_supported(dll, name),
                "{dll}!{name}"
            );
        }
    }
}

#[test]
fn win64_psapi_imports_and_dynamic_kernel_aliases_keep_distinct_visibility() {
    let (mut mem, mut cpu, mut runtime) = startup();
    runtime.register_provider("kernel32.dll");
    runtime.register_provider("psapi.dll");
    let text = STACK + 0x200;
    let output = STACK + 0x300;
    let needed = STACK + 0x380;
    for dll in ["kernel32.dll", "kernelbase.dll", "psapi.dll"] {
        for name in ["EnumProcessModules", "K32EnumProcessModules"] {
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    name,
                    &[u64::MAX, output, 0xdeadbeef00000008, needed],
                ),
                1
            );
            assert!(mem.read_int(needed, 4).unwrap() >= 16);
        }
    }
    for (dll, name, visible) in [
        ("kernel32.dll", "K32EnumProcessModules", true),
        ("kernel32.dll", "EnumProcessModules", false),
        ("psapi.dll", "K32EnumProcessModules", false),
        ("psapi.dll", "EnumProcessModules", false),
    ] {
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetModuleHandleA",
            &[text],
        );
        assert_ne!(module, 0);
        mem.write(text, format!("{name}\0").as_bytes()).unwrap();
        let address = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[module, text],
        );
        assert_eq!(address != 0, visible);
        if !visible {
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
                127
            );
        }
    }
    // The legacy gateway registry also admits an export that was already bound
    // by a static import, even when a fresh provider lookup would reject it.
    for (dll, name) in [
        ("kernel32.dll", "EnumProcessModules"),
        ("psapi.dll", "EnumProcessModules"),
        ("psapi.dll", "K32EnumProcessModules"),
    ] {
        let bound = runtime.register(dll, name);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetModuleHandleA",
            &[text],
        );
        mem.write(text, format!("{name}\0").as_bytes()).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetProcAddress",
                &[module, text],
            ),
            bound
        );
    }
}

#[test]
fn win64_module_queries_keep_dword_sizes_wrong_dll_and_saved_return_frame() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x200;
    let count = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetModuleFileNameA",
        &[0, output, 32],
    );
    assert_ne!(count, 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetModuleFileNameA",
            &[0, output, 0xdeadbeef00000020],
        ),
        count
    );
    let sp = STACK + 0x1f88;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetModuleFileNameA",
            &[0, sp, 32],
        ),
        count
    );
    assert_eq!(cpu.rip, BASE);
    assert_eq!(cpu.regs[4], sp + 8);
    let before = cpu.clone();
    assert!(runtime
        .dispatch("user32.dll", "GetModuleHandleA", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
}

#[test]
fn pe32plus_loads_high_addresses_dir64_iat_and_rejects_bad_ranges() {
    let mut b = fixture();
    let opt = 0x98;
    put32(&mut b, opt + 112 + 5 * 8, 0x1100);
    put32(&mut b, opt + 116 + 5 * 8, 12);
    put32(&mut b, 0x300, 0x1000);
    put32(&mut b, 0x304, 12);
    put16(&mut b, 0x308, 0xa080);
    put64(&mut b, 0x280, BASE + 0x1234);
    put32(&mut b, opt + 112 + 8, 0x1140);
    put32(&mut b, opt + 116 + 8, 40);
    put32(&mut b, 0x340, 0x1180);
    put32(&mut b, 0x34c, 0x11c0);
    put32(&mut b, 0x350, 0x1190);
    put64(&mut b, 0x380, 0x11d0);
    b[0x3c0..0x3cd].copy_from_slice(b"kernel32.dll\0");
    b[0x3d2..0x3de].copy_from_slice(b"ExitProcess\0");
    let pe = Pe64::parse(&b).unwrap();
    let imports = pe.imports(&b).unwrap();
    assert_eq!(imports[0].functions[0].name.as_deref(), Some("ExitProcess"));
    let relocated = BASE + 0x200000;
    let mem = pe
        .load(&b, relocated, |dll, f| {
            assert_eq!(dll, "kernel32.dll");
            assert_eq!(f.iat_rva, 0x1190);
            Ok(0x7ffe12345678)
        })
        .unwrap();
    assert_eq!(
        mem.read_int(relocated + 0x1080, 8).unwrap(),
        relocated + 0x1234
    );
    assert_eq!(mem.read_int(relocated + 0x1190, 8).unwrap(), 0x7ffe12345678);
    assert!(mem.fetch(relocated, &mut [0]).is_err());
    let mut code = [0; 2];
    mem.fetch(relocated + 0x1000, &mut code).unwrap();
    assert_eq!(code, [0x49, 0xb8]);
    put16(&mut b, 0x308, 0x3080);
    assert!(pe.load(&b, relocated, |_, _| Ok(0)).is_err());
    put32(&mut b, 0x98 + 0xf0 + 12, 0x100);
    assert!(Pe64::parse(&b).is_err());
    for n in [0, 63, 128, 200, 511] {
        assert!(Pe64::parse(&b[..n]).is_err());
    }
}

#[test]
fn win64_get_proc_address_null_module_resolves_main_and_missing_exports_return_null() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let symbol = STACK + 0x200;
    mem.write(symbol, b"Exported\0").unwrap();
    let mut exports = wire::x64::pe::Exports64::default();
    exports.names.insert("Exported".into(), BASE + 0x2345);
    exports.ordinals.insert(7, BASE + 0x2345);
    runtime.register_module("probe.exe", BASE, exports);
    for module in [0, BASE] {
        for name in [symbol, 7] {
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    "GetProcAddress",
                    &[module, name]
                ),
                BASE + 0x2345
            );
        }
    }
    for (module, error) in [(0, 127), (BASE, 127), (0x123456780, 126)] {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetProcAddress",
                &[module, 8]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            error
        );
    }
}

#[test]
fn win64_dynamic_kernel_exports_are_available_before_static_gateway_registration() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let text = STACK + 0x200;
    runtime.register_provider("kernel32.dll");
    mem.write(text, b"kernel32.dll\0").unwrap();
    let module = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetModuleHandleA",
        &[text],
    );
    for name in [
        "GetConsoleCP",
        "GetThreadContext",
        "GetFileAttributesExW",
        "CreateWaitableTimerExW",
        "SetThreadErrorMode",
        "GetThreadErrorMode",
    ] {
        assert!(runtime
            .gateways
            .values()
            .all(|n| !n.ends_with(&format!("!{name}"))));
        mem.write(text, format!("{name}\0").as_bytes()).unwrap();
        let p = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[module, text],
        );
        assert_ne!(p, 0, "{name}");
        assert_eq!(runtime.gateways[&p], format!("kernel32.dll!{name}"));
    }
    for name in [
        "CoCreateInstance",
        "BCryptGenRandom",
        "MessageBoxW",
        "TerminateProcess",
        "SetHandleCount",
        "GetCurrentPackageId",
        "DoesNotExist",
    ] {
        mem.write(text, format!("{name}\0").as_bytes()).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetProcAddress",
                &[module, text]
            ),
            0,
            "{name}"
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            127
        );
    }
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetConsoleCP", &[]),
        932
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetThreadErrorMode",
            &[0x8000, text]
        ),
        1
    );
    assert_eq!(mem.read_int(text, 4).unwrap(), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetThreadErrorMode", &[]),
        0x8000
    );
}

#[test]
fn amd64_dll_directory_changes_real_module_search_preserves_faults_and_confines_paths() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x590000000;
    const DATA: u64 = CODE + 4096;
    const CHILD: u64 = 0x1a0000000;
    fn mov(code: &mut Vec<u8>, r: u8, value: u64) {
        code.extend([0x48 | (r >> 3), 0xb8 | (r & 7)]);
        code.extend(value.to_le_bytes());
    }
    let dir = std::env::temp_dir().join(format!("wire-dll-dir64-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("plugins")).unwrap();
    std::fs::create_dir_all(dir.join("cwd")).unwrap();
    std::fs::write(dir.join("main.exe"), fixture()).unwrap();
    let mut child = fixture();
    put16(&mut child, 0x96, 0x2002);
    put64(&mut child, 0x98 + 24, CHILD);
    child[0x200..0x206].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc3]);
    std::fs::write(dir.join("plugins/choice.dll"), &child).unwrap();
    std::fs::write(dir.join("cwd/cwdonly.dll"), &child).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    let original = p.cpu.clone();
    p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
    p.memory.write(DATA, b"C:\\cwd\0").unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "SetCurrentDirectoryA",
            &[DATA]
        ),
        1
    );
    p.memory.write(DATA, b"C:\\plugins\0").unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "SetDllDirectoryA",
            &[DATA]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetDllDirectoryW",
            &[0, 0]
        ),
        11
    );
    p.memory.write(DATA + 128, &[0xcc; 32]).unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetDllDirectoryW",
            &[10, DATA + 128]
        ),
        11
    );
    assert_eq!(
        p.memory.read_int(DATA + 128, 8).unwrap(),
        0xcccccccccccccccc
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetDllDirectoryW",
            &[11, DATA + 128]
        ),
        10
    );
    let mut wide = [0; 22];
    p.memory.read(DATA + 128, &mut wide).unwrap();
    let expected: Vec<u8> = "C:\\plugins\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(&wide[..], &expected);
    p.memory.write(DATA, b"..\\..\\outside\0").unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "SetDllDirectoryA",
            &[DATA]
        ),
        0
    );
    p.cpu.regs[4] = STACK + 0x1f88;
    p.memory.write_int(p.cpu.regs[4], 8, BASE).unwrap();
    p.cpu.regs[1] = CODE + 8192;
    let before = p.cpu.clone();
    let calls = p.runtime.api_calls;
    assert!(p
        .runtime
        .dispatch(
            "kernel32.dll",
            "SetDllDirectoryW",
            &mut p.cpu,
            &mut p.memory
        )
        .is_err());
    assert_eq!(p.cpu, before);
    assert_eq!(p.runtime.api_calls, calls);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetDllDirectoryA",
            &[32, DATA + 128]
        ),
        10
    );

    p.memory.write(DATA, b"choice\0").unwrap(); // Actual load adds the .dll extension.
    let load = p.runtime.register("kernel32.dll", "LoadLibraryA");
    let mut code = vec![0x48, 0x83, 0xec, 0x28];
    mov(&mut code, 1, DATA);
    mov(&mut code, 0, load);
    code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
    p.memory.write(CODE, &code).unwrap();
    p.cpu = original;
    p.cpu.rip = CODE;
    assert_eq!(p.run(1000).unwrap(), Outcome::Returned(CHILD));
    assert!(p
        .modules
        .iter()
        .any(|(name, base)| name == "choice.dll" && *base == CHILD));
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "psapi.dll",
            "GetModuleFileNameExA",
            &[u64::MAX, CHILD, DATA + 128, 64]
        ),
        21
    );
    let mut loaded_path = [0; 22];
    p.memory.read(DATA + 128, &mut loaded_path).unwrap();
    assert_eq!(&loaded_path, b"C:\\plugins\\choice.dll\0");
    // NULL restores CWD search; an empty string removes CWD search.
    p.memory.write(DATA, &[0]).unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "SetDllDirectoryA",
            &[DATA]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetDllDirectoryA",
            &[0, 0]
        ),
        0
    );
    p.memory.write(DATA + 256, b"cwdonly.dll\0").unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "LoadLibraryA",
            &[DATA + 256]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "SetDllDirectoryW",
            &[0]
        ),
        1
    );
    p.memory.write(DATA, b"cwdonly.dll\0").unwrap();
    p.cpu.regs[4] = STACK + 0x1f88;
    p.memory.write_int(p.cpu.regs[4], 8, BASE).unwrap();
    p.cpu.regs[1] = DATA;
    match p
        .runtime
        .dispatch("kernel32.dll", "LoadLibraryA", &mut p.cpu, &mut p.memory)
        .unwrap()
    {
        wire::x64::runtime::Control::LoadModule { path, .. } => {
            assert_eq!(path, "C:\\cwd\\cwdonly.dll")
        }
        other => panic!("CWD search was not restored: {other:?}"),
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn amd64_altered_and_system_search_load_real_dependency_code_without_changing_process_directory() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x5a0000000;
    const DATA: u64 = CODE + 4096;
    const PARENT: u64 = 0x1a0000000;
    const DEP: u64 = 0x1b0000000;
    fn dependency(value: u32) -> Vec<u8> {
        let mut b = fixture();
        put16(&mut b, 0x96, 0x2002);
        put64(&mut b, 0x98 + 24, DEP);
        b[0x200..0x206].copy_from_slice(&[0xb8, 1, 0, 0, 0, 0xc3]);
        put32(&mut b, 0x98 + 112, 0x1100);
        put32(&mut b, 0x98 + 116, 64);
        for (offset, word) in [
            (16, 1),
            (20, 1),
            (24, 1),
            (28, 0x1184),
            (32, 0x1188),
            (36, 0x118c),
        ] {
            put32(&mut b, 0x300 + offset, word);
        }
        put32(&mut b, 0x384, 0x1200);
        put32(&mut b, 0x388, 0x11b0);
        b[0x3b0..0x3b6].copy_from_slice(b"Value\0");
        b[0x400] = 0xb8;
        put32(&mut b, 0x401, value);
        b[0x405] = 0xc3;
        b
    }
    let dir = std::env::temp_dir().join(format!("wire-altered64-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("plugins")).unwrap();
    std::fs::create_dir_all(dir.join("Windows/System32")).unwrap();
    std::fs::write(dir.join("main.exe"), fixture()).unwrap();
    for (path, value) in [
        ("dep.dll", 11),
        ("plugins/dep.dll", 22),
        ("Windows/System32/dep.dll", 33),
    ] {
        std::fs::write(dir.join(path), dependency(value)).unwrap();
    }
    let mut parent = fixture();
    put16(&mut parent, 0x96, 0x2002);
    put64(&mut parent, 0x98 + 24, PARENT);
    put32(&mut parent, 0x98 + 120, 0x1100);
    put32(&mut parent, 0x98 + 124, 40);
    put32(&mut parent, 0x300, 0x1180);
    put32(&mut parent, 0x30c, 0x11c0);
    put32(&mut parent, 0x310, 0x1190);
    put64(&mut parent, 0x380, 0x11d0);
    parent[0x3c0..0x3c8].copy_from_slice(b"dep.dll\0");
    parent[0x3d2..0x3d8].copy_from_slice(b"Value\0");
    // Parent DllMain calls the actual dependency, retaining its result in .text data.
    parent[0x200..0x21b].copy_from_slice(&[
        0x48, 0x83, 0xec, 0x28, 0xff, 0x15, 0x86, 1, 0, 0, 0x48, 0x89, 0x05, 0xef, 2, 0, 0, 0xb8,
        1, 0, 0, 0, 0x48, 0x83, 0xc4, 0x28, 0xc3,
    ]);
    std::fs::write(dir.join("plugins/parent.dll"), parent).unwrap();
    for (flags, expected) in [(0, 11), (8, 22), (0x800, 33)] {
        let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
        p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
        p.memory.write(DATA, b"C:\\plugins\\parent.dll\0").unwrap();
        let gateway = p.runtime.register("kernel32.dll", "LoadLibraryExA");
        let mut code = vec![0x48, 0x83, 0xec, 0x28, 0x48, 0xb9];
        code.extend(DATA.to_le_bytes());
        code.extend([0x31, 0xd2, 0x41, 0xb8]);
        code.extend((flags as u32).to_le_bytes());
        code.extend([0x48, 0xb8]);
        code.extend(gateway.to_le_bytes());
        code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(CODE, &code).unwrap();
        p.cpu.rip = CODE;
        loop {
            match p.run(p.cpu.work + 1).unwrap() {
                Outcome::Budget => assert!(p.cpu.work < 500),
                result => {
                    assert_eq!(result, Outcome::Returned(PARENT));
                    break;
                }
            }
        }
        assert_eq!(p.memory.read_int(PARENT + 0x1300, 8).unwrap(), expected);
        assert_eq!(p.memory.read_int(PARENT + 0x1190, 8).unwrap(), DEP + 0x1200);
        assert_eq!(
            startup_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "GetDllDirectoryA",
                &[0, 0]
            ),
            0
        );
        p.memory.write(DATA, b"parent.dll\0").unwrap();
        assert_eq!(
            startup_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "LoadLibraryExA",
                &[DATA, 0, 8]
            ),
            0
        );
        assert_eq!(
            startup_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "GetLastError",
                &[]
            ),
            87
        );
        p.memory.write(DATA, b"C:\\plugins\\parent.dll\0").unwrap();
        assert_eq!(
            startup_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "LoadLibraryExA",
                &[DATA, 0, 0x808]
            ),
            0
        );
    }
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    p.memory.map(DATA, 4096, READ | WRITE).unwrap();
    p.memory
        .write(DATA, b"C:\\Windows\\System32\\ntdll.dll\0")
        .unwrap();
    let handle = startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "LoadLibraryExA",
        &[DATA, 0, 8],
    );
    assert_ne!(handle, 0);
    assert!(!dir.join("Windows/System32/ntdll.dll").exists());
    p.memory.write(DATA, b"C:\\plugins\\ntdll.dll\0").unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "LoadLibraryExA",
            &[DATA, 0, 8]
        ),
        0
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn win64_loaded_module_queries_messages_and_disabled_etw_keep_real_lifetimes() {
    use wire::x64::process::Process64;
    let dir = std::env::temp_dir().join(format!("wire-modulequery64-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.exe"), fixture()).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    let out = STACK + 0x100;
    let needed = STACK + 0x180;
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "K32EnumProcessModules",
            &[u64::MAX, out, 8, needed]
        ),
        1
    );
    assert_eq!(p.memory.read_int(out, 8).unwrap(), BASE);
    assert_eq!(p.memory.read_int(needed, 4).unwrap(), 24); // EXE, implicit kernel32 and ntdll.
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "K32EnumProcessModulesEx",
            &[u64::MAX, out, 8, needed, 1]
        ),
        1
    );
    assert_eq!(p.memory.read_int(needed, 4).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "K32GetModuleInformation",
            &[u64::MAX, BASE, out, 24]
        ),
        1
    );
    assert_eq!(p.memory.read_int(out, 8).unwrap(), BASE);
    assert_eq!(p.memory.read_int(out + 8, 4).unwrap(), 0x2000);
    assert_eq!(p.memory.read_int(out + 16, 8).unwrap(), BASE + 0x1000);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "psapi.dll",
            "GetModuleBaseNameA",
            &[u64::MAX, BASE, out, 32]
        ),
        8
    );
    let mut name = [0; 9];
    p.memory.read(out, &mut name).unwrap();
    assert_eq!(&name, b"main.exe\0");
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "K32GetModuleFileNameExA",
            &[u64::MAX, BASE, out, 32]
        ),
        11
    );
    let mut path = [0; 12];
    p.memory.read(out, &mut path).unwrap();
    assert_eq!(&path, b"C:\\main.exe\0");
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetModuleHandleExW",
            &[6, BASE + 0x1200, needed]
        ),
        1
    );
    assert_eq!(p.memory.read_int(needed, 8).unwrap(), BASE);
    assert!(p
        .runtime
        .dispatch(
            "kernel32.dll",
            "FreeLibrary",
            &mut {
                let mut cpu = p.cpu.clone();
                cpu.regs[4] = STACK + 0x1f88;
                p.memory.write_int(cpu.regs[4], 8, BASE).unwrap();
                cpu.regs[1] = BASE;
                cpu
            },
            &mut p.memory
        )
        .is_err()); // UNCHANGED_REFCOUNT does not create a releasable reference.
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetModuleHandleExW",
            &[4, BASE + 0x1200, needed]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FreeLibrary",
            &[BASE]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetModuleHandleExW",
            &[5, BASE + 0x1200, needed]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FreeLibrary",
            &[BASE]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FreeLibrary",
            &[BASE]
        ),
        1
    ); // A pinned image stays loaded.
    let count = startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "FormatMessageW",
        &[0x1300, 0, 127, 0x400, out, 128, 0],
    );
    assert_eq!(count, 45);
    let text = p.memory.read_int(out, 8).unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "LocalSize",
            &[text]
        ),
        256
    );
    assert_eq!(p.memory.read_int(text + count * 2, 2).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "LocalFree",
            &[text]
        ),
        0
    );
    p.memory.write_int(out, 8, 0xfeed).unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FormatMessageW",
            &[0x1200, 0, 999999, 0, out, 128, 0]
        ),
        0
    );
    assert_eq!(p.memory.read_int(out, 8).unwrap(), 0xfeed);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        317
    );
    let guid = STACK + 0x200;
    p.memory.write(guid, &[0x42; 16]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "advapi32.dll",
            "EventRegister",
            &[guid, 0x510000000, 0xdeadbeef12345678, out]
        ),
        0
    );
    let registration = p.memory.read_int(out, 8).unwrap();
    assert_ne!(registration, 0);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "advapi32.dll",
            "EventEnabled",
            &[registration, guid]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "advapi32.dll",
            "EventProviderEnabled",
            &[registration, 5, u64::MAX]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "advapi32.dll",
            "EventWrite",
            &[registration, guid, 0, 0]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "advapi32.dll",
            "EventUnregister",
            &[registration]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "advapi32.dll",
            "EventWrite",
            &[registration, guid, 0, 0]
        ),
        6
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        317
    ); // ETW returns its error directly.
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn amd64_static_tls_and_delay_iat_are_high_address_runtime_data() {
    let mut b = fixture();
    let opt = 0x98;
    put32(&mut b, opt + 112 + 9 * 8, 0x1100);
    put32(&mut b, opt + 116 + 9 * 8, 40);
    put64(&mut b, 0x300, BASE + 0x11c0);
    put64(&mut b, 0x308, BASE + 0x11c4);
    put64(&mut b, 0x310, BASE + 0x1180);
    put64(&mut b, 0x318, BASE + 0x1190);
    put32(&mut b, 0x320, 12);
    put32(&mut b, 0x324, 5 << 20);
    put64(&mut b, 0x390, BASE + 0x1220);
    put32(&mut b, 0x3c0, 0x12345678);
    b[0x420] = 0xc3;
    let pe = Pe64::parse(&b).unwrap();
    assert!(pe.load(&b, BASE, |_, _| Ok(0)).is_err());
    let mut mem = pe.load_with_runtime(&b, BASE, |_, _| Ok(0)).unwrap();
    mem.map(STACK, 8192, READ | WRITE).unwrap();
    let mut runtime =
        wire::x64::runtime::Runtime64::new(&mut mem, BASE, STACK, 8192, "probe").unwrap();
    let tls = pe.tls(&mem, BASE).unwrap().unwrap();
    assert_eq!(tls.callbacks, vec![BASE + 0x1220]);
    runtime.initialize_tls(&mut mem, &tls).unwrap();
    let vector = mem.read_int(wire::x64::runtime::TEB + 0x58, 8).unwrap();
    let ptr = mem.read_int(vector, 8).unwrap();
    assert!(ptr > u32::MAX as u64);
    assert_eq!(mem.read_int(ptr, 4).unwrap(), 0x12345678);
    assert_eq!(mem.read_int(ptr + 4, 8).unwrap(), 0);
    assert_eq!(mem.read_int(BASE + 0x1180, 4).unwrap(), 0);
    let mut b = fixture();
    put32(&mut b, opt + 112 + 13 * 8, 0x1100);
    put32(&mut b, opt + 116 + 13 * 8, 64);
    for (offset, value) in [(0, 1), (4, 0x1180), (8, 0x11a0), (12, 0x11b0), (16, 0x11d0)] {
        put32(&mut b, 0x300 + offset, value);
    }
    b[0x380..0x38d].copy_from_slice(b"kernel32.dll\0");
    put64(&mut b, 0x3d0, 0x11e0);
    b[0x3e2..0x3f6].copy_from_slice(b"GetCurrentProcessId\0");
    let pe = Pe64::parse(&b).unwrap();
    let delays = pe.delay_imports(&b).unwrap();
    assert_eq!(delays.len(), 1);
    let mut mem = pe.load_with_runtime(&b, BASE, |_, _| Ok(0)).unwrap();
    mem.map(STACK, 8192, READ | WRITE).unwrap();
    let rsp = STACK + 0x1fc8;
    mem.write_int(rsp, 8, BASE).unwrap();
    let mut cpu = Cpu64::new(BASE, rsp);
    let mut runtime =
        wire::x64::runtime::Runtime64::new(&mut mem, BASE, STACK, 8192, "probe").unwrap();
    runtime.install_gateways(Default::default());
    runtime.install_delays(&mut mem, BASE, &delays).unwrap();
    let gateway = mem.read_int(BASE + 0x11b0, 8).unwrap();
    assert!(gateway > u32::MAX as u64);
    assert_eq!(mem.read_int(BASE + 0x11a0, 8).unwrap(), 0);
    cpu.rip = gateway;
    runtime
        .dispatch("kernel32.dll", "GetCurrentProcessId", &mut cpu, &mut mem)
        .unwrap();
    assert_eq!(cpu.regs[0], 1);
    assert_ne!(mem.read_int(BASE + 0x11a0, 8).unwrap(), 0);
    put32(&mut b, 0x300, 0);
    assert!(pe.delay_imports(&b).is_err());
}

#[test]
fn amd64_dll_cli_runs_static_tls_callback_before_real_attach_entry() {
    use std::process::Command;
    let mut b = fixture();
    put16(&mut b, 0x96, 0x2002);
    put32(&mut b, 0x98 + 112 + 9 * 8, 0x1100);
    put32(&mut b, 0x98 + 116 + 9 * 8, 40);
    put64(&mut b, 0x300, BASE + 0x11c0);
    put64(&mut b, 0x308, BASE + 0x11c4);
    put64(&mut b, 0x310, BASE + 0x1180);
    put64(&mut b, 0x318, BASE + 0x1190);
    put64(&mut b, 0x390, BASE + 0x1220);
    put32(&mut b, 0x3c0, 0x12345678);
    let load = [
        0x65, 0x48, 0x8b, 0x04, 0x25, 0x58, 0, 0, 0, 0x48, 0x8b, 0x00,
    ];
    b[0x200..0x20c].copy_from_slice(&load);
    b[0x20c..0x20f].copy_from_slice(&[0x8b, 0x00, 0xc3]);
    b[0x420..0x42c].copy_from_slice(&load);
    b[0x42c..0x42f].copy_from_slice(&[0xff, 0x00, 0xc3]);
    let path = std::env::temp_dir().join(format!("wire-dll64-cli-{}.dll", std::process::id()));
    std::fs::write(&path, b).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wire"))
        .args(["probe64", path.to_str().unwrap(), "--dll"])
        .output()
        .unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains("TLS process attach: 0x140001220"));
    assert!(report.contains("entry returned RAX=0x12345679; retired=8"));
}

#[test]
fn amd64_process_binds_guest_dll_export_and_attaches_before_readonly_exe_iat_call() {
    use wire::x64::process::{Outcome, Process64};
    let mut child = fixture();
    put16(&mut child, 0x96, 0x2002);
    put32(&mut child, 0x98 + 112, 0x1100);
    put32(&mut child, 0x98 + 116, 80);
    for (offset, value) in [
        (16, 1),
        (20, 1),
        (24, 1),
        (28, 0x1180),
        (32, 0x1190),
        (36, 0x11a0),
    ] {
        put32(&mut child, 0x300 + offset, value);
    }
    put32(&mut child, 0x380, 0x1200);
    put32(&mut child, 0x390, 0x11b0);
    child[0x3b0..0x3b6].copy_from_slice(b"Value\0");
    // Both files prefer BASE; this valid ABS-only relocation table allows moving the RIP-relative child.
    put32(&mut child, 0x98 + 112 + 5 * 8, 0x11e0);
    put32(&mut child, 0x98 + 116 + 5 * 8, 12);
    put32(&mut child, 0x3e0, 0x1000);
    put32(&mut child, 0x3e4, 12);
    child[0x200..0x20d].copy_from_slice(&[0xc6, 0x05, 0xf9, 0x02, 0, 0, 1, 0xb8, 1, 0, 0, 0, 0xc3]);
    child[0x400..0x408].copy_from_slice(&[0x0f, 0xb6, 0x05, 0xf9, 0, 0, 0, 0xc3]);
    let mut main = fixture();
    put32(&mut main, 0x98 + 120, 0x1140);
    put32(&mut main, 0x98 + 124, 40);
    put32(&mut main, 0x340, 0x1180);
    put32(&mut main, 0x34c, 0x11c0);
    put32(&mut main, 0x350, 0x1190);
    put64(&mut main, 0x380, 0x11d0);
    main[0x3c0..0x3ca].copy_from_slice(b"child.dll\0");
    main[0x3d2..0x3d8].copy_from_slice(b"Value\0");
    main[0x200..0x20f].copy_from_slice(&[
        0x48, 0x83, 0xec, 0x28, 0xff, 0x15, 0x86, 1, 0, 0, 0x48, 0x83, 0xc4, 0x28, 0xc3,
    ]);
    put32(&mut main, 0x98 + 0xf0 + 36, 0x60000020);
    let dir = std::env::temp_dir().join(format!("wire-linked64-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.exe"), main).unwrap();
    std::fs::write(dir.join("child.dll"), child).unwrap();
    let mut process = Process64::load(&dir.join("main.exe"), false).unwrap();
    let child_base = process
        .modules
        .iter()
        .find(|(name, _)| name == "child.dll")
        .unwrap()
        .1;
    assert_ne!(child_base, BASE);
    assert!(child_base > u32::MAX as u64);
    assert_eq!(
        process.memory.read_int(BASE + 0x1190, 8).unwrap(),
        child_base + 0x1200
    );
    assert!(process.memory.check(BASE + 0x1190, 8, WRITE).is_err());
    assert_eq!(process.run(100).unwrap(), Outcome::Returned(1));
    assert_eq!(
        process
            .stages
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["child.dll entry", "main.exe entry"]
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn amd64_dynamic_load_executes_tls_and_entry_once_and_initializes_existing_thread_tls() {
    use wire::x64::process::{Outcome, Process64};
    const CHILD: u64 = BASE + 0x1000000;
    const CODE: u64 = 0x510000000;
    const WORKER: u64 = CODE + 4096;
    const DATA: u64 = CODE + 8192;
    fn mov(code: &mut Vec<u8>, reg: u8, value: u64) {
        code.extend([0x48 | (reg >> 3), 0xb8 | (reg & 7)]);
        code.extend(value.to_le_bytes());
    }
    fn call(p: &mut Process64, code: &mut Vec<u8>, name: &str, args: &[u64]) {
        for (n, &value) in args.iter().enumerate() {
            mov(code, if n < 4 { [1, 2, 8, 9][n] } else { 0 }, value);
            if n >= 4 {
                code.extend([0x48, 0x89, 0x44, 0x24, 32 + (n as u8 - 4) * 8]);
            }
        }
        mov(code, 0, p.runtime.register("kernel32.dll", name));
        code.extend([0xff, 0xd0]);
    }
    fn store(code: &mut Vec<u8>, address: u64) {
        mov(code, 7, address);
        code.extend([0x48, 0x89, 0x07]);
    }
    let dir = std::env::temp_dir().join(format!("wire-dynamic64-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("engine")).unwrap();
    std::fs::write(dir.join("main.exe"), fixture()).unwrap();
    let mut child = fixture();
    put16(&mut child, 0x96, 0x2002);
    put64(&mut child, 0x98 + 24, CHILD);
    put32(&mut child, 0x98 + 112, 0x1100);
    put32(&mut child, 0x98 + 116, 64);
    for (offset, value) in [
        (16, 1),
        (20, 1),
        (24, 1),
        (28, 0x1184),
        (32, 0x1188),
        (36, 0x118c),
    ] {
        put32(&mut child, 0x300 + offset, value);
    }
    put32(&mut child, 0x384, 0x1200);
    put32(&mut child, 0x388, 0x11b0);
    child[0x3b0..0x3b6].copy_from_slice(b"Value\0");
    put32(&mut child, 0x98 + 112 + 9 * 8, 0x1140);
    put32(&mut child, 0x98 + 116 + 9 * 8, 40);
    put64(&mut child, 0x340, CHILD + 0x11c0);
    put64(&mut child, 0x348, CHILD + 0x11c4);
    put64(&mut child, 0x350, CHILD + 0x1180);
    put64(&mut child, 0x358, CHILD + 0x1190);
    put32(&mut child, 0x360, 4);
    put64(&mut child, 0x390, CHILD + 0x1220);
    put32(&mut child, 0x3c0, 0x1234);
    let tls_load = [
        0x65, 0x48, 0x8b, 0x04, 0x25, 0x58, 0, 0, 0, 0x48, 0x8b, 0x00,
    ];
    let mut entry = vec![0x83, 0xfa, 1, 0x75, 15]; // Only process attach mutates the loading thread.
    entry.extend(tls_load);
    entry.extend([0x83, 0x00, 0x10, 0xb8, 1, 0, 0, 0, 0xc3]);
    child[0x200..0x200 + entry.len()].copy_from_slice(&entry);
    let mut value = tls_load.to_vec();
    value.extend([0x8b, 0x00, 0xc3]);
    child[0x400..0x400 + value.len()].copy_from_slice(&value);
    let mut callback = tls_load.to_vec();
    callback.extend([0xff, 0x00, 0xc3]);
    child[0x420..0x420 + callback.len()].copy_from_slice(&callback);
    std::fs::write(dir.join("engine/child.dll"), &child).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    p.memory
        .map(CODE, 3 * 4096, READ | WRITE | EXECUTE)
        .unwrap();
    let path: Vec<_> = "C:\\engine\\child.dll"
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    p.memory.write(DATA + 0x100, &path).unwrap();
    p.memory.write(DATA + 0x180, b"Value\0").unwrap();
    let mut code = vec![0x48, 0x83, 0xec, 0x38];
    call(
        &mut p,
        &mut code,
        "CreateThread",
        &[0, 0, WORKER, DATA, 4, 0],
    );
    code.extend([0x49, 0x89, 0xc4]); // r12 = suspended thread
    call(&mut p, &mut code, "SwitchToThread", &[]);
    store(&mut code, DATA + 32);
    call(&mut p, &mut code, "LoadLibraryW", &[DATA + 0x100]);
    store(&mut code, DATA + 8);
    call(&mut p, &mut code, "GetProcAddress", &[CHILD, DATA + 0x180]);
    code.extend([0xff, 0xd0]);
    store(&mut code, DATA + 16);
    call(&mut p, &mut code, "LoadLibraryW", &[DATA + 0x100]);
    store(&mut code, DATA + 24);
    code.extend([0x4c, 0x89, 0xe1]); // rcx = r12
    call(&mut p, &mut code, "ResumeThread", &[]);
    call(&mut p, &mut code, "SwitchToThread", &[]);
    store(&mut code, DATA + 40);
    code.extend([0x4c, 0x89, 0xe1]);
    mov(&mut code, 2, u32::MAX as u64);
    call(&mut p, &mut code, "WaitForSingleObject", &[]);
    code.extend([0xb8, 1, 0, 0, 0, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
    p.memory.write(CODE, &code).unwrap();
    let mut worker = tls_load.to_vec();
    worker.extend([0x8b, 0x00]);
    store(&mut worker, DATA);
    worker.push(0xc3);
    p.memory.write(WORKER, &worker).unwrap();
    p.cpu.rip = CODE;
    let initial_rsp = p.cpu.regs[4];
    loop {
        let budget = p.cpu.work + 1;
        match p.run(budget).unwrap() {
            Outcome::Budget => assert!(p.cpu.work < 10000),
            result => {
                assert_eq!(result, Outcome::Returned(1));
                break;
            }
        }
    }
    assert_eq!(p.cpu.regs[4], initial_rsp + 8);
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), 0x1235);
    assert_eq!(p.memory.read_int(DATA + 8, 8).unwrap(), CHILD);
    assert_eq!(p.memory.read_int(DATA + 16, 8).unwrap(), 0x1245);
    assert_eq!(p.memory.read_int(DATA + 24, 8).unwrap(), CHILD);
    assert_eq!(p.memory.read_int(DATA + 32, 8).unwrap(), 0);
    assert_eq!(p.memory.read_int(DATA + 40, 8).unwrap(), 1);
    assert_eq!(
        p.stages
            .iter()
            .filter(|(s, _)| s == "child.dll dynamic entry")
            .count(),
        1
    );
    assert_eq!(
        p.stages
            .iter()
            .filter(|(s, _)| s == "child.dll TLS thread 2")
            .count(),
        1
    );
    assert_eq!(
        p.modules.iter().filter(|(s, _)| s == "child.dll").count(),
        1
    );
    // A FALSE DllMain is an explicit unsupported rollback boundary, never successful loading.
    child[0x200..0x203].copy_from_slice(&[0x31, 0xc0, 0xc3]);
    std::fs::write(dir.join("engine/child.dll"), &child).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    p.memory
        .map(CODE, 3 * 4096, READ | WRITE | EXECUTE)
        .unwrap();
    p.memory.write(DATA + 0x100, &path).unwrap();
    let mut code = vec![0x48, 0x83, 0xec, 0x28];
    call(&mut p, &mut code, "LoadLibraryW", &[DATA + 0x100]);
    code.extend([0x48, 0x83, 0xc4, 0x28, 0xc3]);
    p.memory.write(CODE, &code).unwrap();
    p.cpu.rip = CODE;
    assert!(p
        .run(1000)
        .unwrap_err()
        .contains("DLL_PROCESS_ATTACH returned FALSE"));
    assert!(!p
        .runtime
        .trace
        .iter()
        .any(|line| line.contains("LoadLibraryW")));
    std::fs::remove_dir_all(dir).unwrap();
}
