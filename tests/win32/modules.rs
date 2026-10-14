//! Existing behavioral tests grouped by modules; expectations remain independent of API catalogs.

use super::*;

#[test]
fn module_provider_names_keep_unknown_dll_abi_and_filename_return_alias() {
    let mut v = Vm::new();
    let sp = STACK_BASE + STACK_SIZE - 256;
    for dll in ["kernelbase.dll", "user32.dll"] {
        v.k("SetLastError", &[0x1234]);
        assert_eq!(v.call(dll, "GetModuleHandleA", &[0]), 0);
        assert_eq!(v.cpu.esp(), sp + 4);
        assert_eq!(v.k("GetLastError", &[]), 120);
    }
    assert_eq!(v.k("GetModuleHandleA", &[0]), 0x400000);
    assert_eq!(v.cpu.esp(), sp + 8);

    // PE32 reads the return slot after the filename output, so aliasing it
    // preserves the existing overwritten-return behavior.
    assert_eq!(v.k("GetModuleFileNameA", &[0, sp, 32]), 11);
    assert_eq!(v.cpu.eip, u32::from_le_bytes(*b"C:\\t"));
    assert_eq!(v.cpu.esp(), sp + 16);
}

fn test_tls_dll(attach_ok: bool) -> Vec<u8> {
    let mut bytes = test_dll("kernel32.dll!GetCurrentProcessId", attach_ok, 0);
    let put = |bytes: &mut [u8], rva: usize, data: &[u8]| {
        let at = 0x200 + rva - 0x1000;
        bytes[at..at + data.len()].copy_from_slice(data);
    };
    bytes[0x140..0x144].copy_from_slice(&0x1800u32.to_le_bytes());
    bytes[0x144..0x148].copy_from_slice(&24u32.to_le_bytes());
    for (i, value) in [
        0x10001900u32,
        0x10001904,
        0x10001920,
        0x10001930,
        12,
        0x500000,
    ]
    .into_iter()
    .enumerate()
    {
        put(&mut bytes, 0x1800 + i * 4, &value.to_le_bytes());
    }
    put(&mut bytes, 0x1900, &37u32.to_le_bytes());
    let mut access = vec![0x64, 0x8b, 0x0d, 0x2c, 0, 0, 0, 0xa1];
    access.extend(0x10001920u32.to_le_bytes());
    access.extend([0x8b, 0x04, 0x81, 0x8b, 0x10]);
    let mut entry = access.clone();
    entry.extend([0x89, 0x15]);
    entry.extend((DATA + 0x720).to_le_bytes());
    entry.push(0xb8);
    entry.extend(u32::from(attach_ok).to_le_bytes());
    entry.extend([0xc2, 12, 0]);
    put(&mut bytes, 0x1000, &entry);
    access.extend([0xff, 0x00, 0x89, 0xd0, 0xc3]); // return previous TLS value, increment it
    put(&mut bytes, 0x1080, &access);
    let mut reloc = Vec::from(0x1000u32.to_le_bytes());
    reloc.extend(20u32.to_le_bytes());
    for offset in [0x3008u16, 0x3088, 0x3800, 0x3804, 0x3808, 0x380c] {
        reloc.extend(offset.to_le_bytes());
    }
    put(&mut bytes, 0x1700, &reloc);
    bytes[0x124..0x128].copy_from_slice(&(reloc.len() as u32).to_le_bytes());
    bytes
}

#[test]
fn version_resources_query_fixed_translation_and_ansi_unicode_nested_strings() {
    fn block(key: &str, text: bool, value: Vec<u8>, children: Vec<Vec<u8>>) -> Vec<u8> {
        let mut b = vec![0; 6];
        b[2..4].copy_from_slice(&((value.len() / if text { 2 } else { 1 }) as u16).to_le_bytes());
        b[4..6].copy_from_slice(&u16::from(text).to_le_bytes());
        b.extend(key.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
        b.resize((b.len() + 3) & !3, 0);
        b.extend(value);
        for child in children {
            b.resize((b.len() + 3) & !3, 0);
            b.extend(child);
        }
        let len = b.len() as u16;
        b[..2].copy_from_slice(&len.to_le_bytes());
        b
    }
    let mut v = Vm::new();
    let leaf = block(
        "CompanyName",
        true,
        "ソフト"
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect(),
        vec![],
    );
    let strings = block(
        "StringFileInfo",
        true,
        vec![],
        vec![block("041103a4", true, vec![], vec![leaf])],
    );
    let vars = block(
        "VarFileInfo",
        true,
        vec![],
        vec![block(
            "Translation",
            false,
            [0x411u16.to_le_bytes(), 932u16.to_le_bytes()].concat(),
            vec![],
        )],
    );
    let mut fixed = vec![0; 52];
    fixed[..4].copy_from_slice(&0xfeef04bdu32.to_le_bytes());
    let blob = block("VS_VERSION_INFO", false, fixed, vec![strings, vars]);
    let mut exe = test_dll("kernel32.dll!GetCurrentProcessId", true, 0);
    exe[0x108..0x10c].copy_from_slice(&0x1900u32.to_le_bytes());
    exe[0x10c..0x110].copy_from_slice(&0x100u32.to_le_bytes());
    let root = 0xb00;
    for off in [0, 24, 48] {
        exe[root + off + 14..root + off + 16].copy_from_slice(&1u16.to_le_bytes());
    }
    for (off, n) in [
        (16, 16u32),
        (20, 0x80000018),
        (40, 1),
        (44, 0x80000030),
        (64, 0x411),
        (68, 72),
        (72, 0x1a00),
        (76, blob.len() as u32),
    ] {
        exe[root + off..root + off + 4].copy_from_slice(&n.to_le_bytes());
    }
    exe[0xc00..0xc00 + blob.len()].copy_from_slice(&blob);
    std::fs::write(v.dir.join("version.dll"), &exe).unwrap();
    v.mem.write_bytes(DATA, b"C:\\version.dll\0").unwrap();
    let size = v.call(
        "version.dll",
        "GetFileVersionInfoSizeA",
        &[DATA, DATA + 0x100],
    );
    assert!(size >= blob.len() as u32);
    assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 0);
    assert_eq!(
        v.call(
            "version.dll",
            "GetFileVersionInfoA",
            &[DATA, 0, size, DATA + 0x1000]
        ),
        1
    );
    for (path, len) in [("\\", 52), ("\\VarFileInfo\\Translation", 4)] {
        v.wide(DATA + 0x200, path);
        assert_eq!(
            v.call(
                "version.dll",
                "VerQueryValueW",
                &[DATA + 0x1000, DATA + 0x200, DATA + 0x100, DATA + 0x104]
            ),
            1
        );
        assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), len);
    }
    let path = "\\StringFileInfo\\041103a4\\CompanyName";
    v.wide(DATA + 0x200, path);
    assert_eq!(
        v.call(
            "version.dll",
            "VerQueryValueW",
            &[DATA + 0x1000, DATA + 0x200, DATA + 0x100, DATA + 0x104]
        ),
        1
    );
    let wide = v.mem.read_u32(DATA + 0x100).unwrap();
    assert_eq!(v.mem.read_wstr(wide, 10).unwrap(), "ソフト");
    assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 4);
    v.mem
        .write_bytes(DATA + 0x200, format!("{path}\0").as_bytes())
        .unwrap();
    assert_eq!(
        v.call(
            "version.dll",
            "VerQueryValueA",
            &[DATA + 0x1000, DATA + 0x200, DATA + 0x100, DATA + 0x104]
        ),
        1
    );
    let ansi = v.mem.read_u32(DATA + 0x100).unwrap();
    assert_eq!(
        wire::file_api::read_name(&v.mem, ansi, false).unwrap(),
        "ソフト"
    );
    assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 7);
    assert_eq!(v.mem.read_wstr(wide, 10).unwrap(), "ソフト");
    v.mem.write_u16(DATA + 0x1000, 5).unwrap();
    assert_eq!(
        v.call(
            "version.dll",
            "VerQueryValueA",
            &[DATA + 0x1000, DATA + 0x200, DATA + 0x100, DATA + 0x104]
        ),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 0);
}

#[test]
fn large_dll_and_resource_only_dll_preserve_heap_and_reuse_loaded_basenames() {
    let mut v = Vm::new();
    let heap = v.state.process_heap;
    let allocation = v.k("HeapAlloc", &[heap, 0, 16]);
    v.mem.write_u32(allocation, 0x1234).unwrap();
    let mut large = test_dll("kernel32.dll!GetCurrentProcessId", true, 42);
    large[0xd0..0xd4].copy_from_slice(&0x4138000u32.to_le_bytes());
    std::fs::create_dir(v.dir.join("dll")).unwrap();
    let path = v.dir.join("dll/large.dll");
    std::fs::write(&path, &large).unwrap();
    v.mem.write_bytes(DATA, b"C:\\dll\\large.dll\0").unwrap();
    let h = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(h, 0);
    assert_eq!(v.mem.read_u32(allocation).unwrap(), 0x1234);
    assert_eq!(
        v.mem.virtual_query(h + 0x4137000).unwrap().allocation_base,
        h
    );
    std::fs::remove_file(&path).unwrap();
    v.mem.write_bytes(DATA, b"LARGE.DLL\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), h);
    assert_eq!(v.k("FreeLibrary", &[h]), 1);
    assert!(v.mem.is_committed(h));
    assert_eq!(v.k("FreeLibrary", &[h]), 1);
    assert!(!v.mem.is_committed(h));
    let mut resource = test_dll("kernel32.dll!GetCurrentProcessId", true, 0);
    resource[0xa8..0xac].fill(0);
    resource[0xf8..0x108].fill(0);
    resource[0x120..0x128].fill(0);
    resource[0x19c..0x1a0].copy_from_slice(&0x40000040u32.to_le_bytes());
    std::fs::write(v.dir.join("resource.dll"), resource).unwrap();
    v.mem.write_bytes(DATA, b"resource.dll\0").unwrap();
    let h = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(h, 0);
    assert!(v.mem.read_u32(h + 0x1200).is_ok());
    assert_eq!(
        v.mem.virtual_query(h + 0x1000).unwrap().protect,
        wire::mem::PAGE_READONLY
    );
    assert_eq!(v.k("FreeLibrary", &[h]), 1);
}

#[test]
fn main_static_tls_is_initialized_before_entry_and_reserves_its_loader_index() {
    let mut v = Vm::new();
    let mut bytes = test_tls_dll(true);
    bytes[0x96..0x98].copy_from_slice(&0x102u16.to_le_bytes());
    bytes[0xb4..0xb8].copy_from_slice(&0x400000u32.to_le_bytes());
    for rva in [0x1008usize, 0x1088, 0x1800, 0x1804, 0x1808, 0x180c] {
        let at = 0x200 + rva - 0x1000;
        let va = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        bytes[at..at + 4].copy_from_slice(&(va - 0x10000000 + 0x400000).to_le_bytes());
    }
    let path = v.dir.join("main-tls.exe");
    std::fs::write(&path, &bytes).unwrap();
    let mut main_mem = GuestMem::new().unwrap();
    let mut main_api = ApiTable::new();
    let loaded =
        wire::loader::load_image(&mut main_mem, &mut main_api, &path, "main-tls.exe").unwrap();
    main_mem.commit(CODE, 0x10000).unwrap();
    main_mem.commit(DATA, 0x10000).unwrap();
    v.mem = main_mem;
    v.api = main_api;
    v.state
        .modules
        .register_main(
            &v.mem,
            loaded.image_base,
            path.clone(),
            "C:\\main-tls.exe".into(),
        )
        .unwrap();
    v.state
        .modules
        .setup_tls_thread(
            &mut v.mem,
            &mut v.state.heap,
            v.state.process_heap,
            loaded.teb,
        )
        .unwrap();
    let index = v.mem.read_u32(0x401920).unwrap();
    assert_eq!(index, 0);
    let vector = v.mem.read_u32(loaded.teb + 0x2c).unwrap();
    assert_ne!(vector, 0);
    let block = v.mem.read_u32(vector).unwrap();
    assert_eq!(v.mem.read_u32(block).unwrap(), 37);
    for i in 4..16 {
        assert_eq!(v.mem.read_u8(block + i).unwrap(), 0);
    }
    let f = dll_export(&mut v, loaded.image_base, "GetValue");
    assert!(matches!(
        v.cpu
            .call_guest(
                &mut v.mem,
                &mut v.api,
                &mut v.state,
                &mut v.log,
                &mut v.sched,
                f,
                &[],
                1000
            )
            .unwrap(),
        wire::cpu::GuestCallOutcome::Returned(37)
    ));
    let thread = v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    let teb = v.sched.threads[v.sched.thread_index_by_handle(thread).unwrap()].teb;
    let other = v.mem.read_u32(v.mem.read_u32(teb + 0x2c).unwrap()).unwrap();
    assert_ne!(other, block);
    assert_eq!(v.mem.read_u32(other).unwrap(), 37);
    std::fs::write(v.dir.join("tls.dll"), test_tls_dll(true)).unwrap();
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    let dll = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(dll, 0);
    assert_eq!(v.mem.read_u32(dll + 0x1920).unwrap(), 1);
    assert_eq!(v.mem.read_u32(block).unwrap(), 38);
    v.k("FreeLibrary", &[dll]);
    assert_eq!(v.mem.read_u32(vector).unwrap(), block);
    // Invalid main callback targets are rejected before registration, never ignored.
    bytes[0xb30..0xb34].copy_from_slice(&0x404000u32.to_le_bytes());
    std::fs::write(&path, &bytes).unwrap();
    let mut mem = GuestMem::new().unwrap();
    let mut api = ApiTable::new();
    wire::loader::load_image(&mut mem, &mut api, &path, "main-tls.exe").unwrap();
    assert!(wire::modules::Modules::default()
        .register_main(&mem, 0x400000, path, "C:\\main-tls.exe".into())
        .unwrap_err()
        .contains("TLS"));
}

#[test]
fn dll_static_tls_is_ready_for_dllmain_and_isolated_for_existing_and_new_threads() {
    let mut v = Vm::new();
    let existing = v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    let existing_teb = v.sched.threads[v.sched.thread_index_by_handle(existing).unwrap()].teb;
    std::fs::write(v.dir.join("tls.dll"), test_tls_dll(true)).unwrap();
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    let module = v.k("LoadLibraryA", &[DATA]);
    assert_eq!(module, 0x50000000);
    assert_eq!(v.mem.read_u32(DATA + 0x720).unwrap(), 37); // read by actual guest DllMain
    let function = dll_export(&mut v, module, "GetValue");
    let call = |v: &mut Vm| {
        v.cpu
            .call_guest(
                &mut v.mem,
                &mut v.api,
                &mut v.state,
                &mut v.log,
                &mut v.sched,
                function,
                &[],
                1_000_000,
            )
            .unwrap()
    };
    assert!(matches!(
        call(&mut v),
        wire::cpu::GuestCallOutcome::Returned(37)
    ));
    assert!(matches!(
        call(&mut v),
        wire::cpu::GuestCallOutcome::Returned(38)
    ));
    let new = v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    let new_teb = v.sched.threads[v.sched.thread_index_by_handle(new).unwrap()].teb;
    let index = v.mem.read_u32(module + 0x1920).unwrap();
    let main_vector = v.mem.read_u32(TEB_ADDR + 0x2c).unwrap();
    let main_block = v.mem.read_u32(main_vector + index * 4).unwrap();
    assert_eq!(v.mem.read_u32(main_block).unwrap(), 39);
    for teb in [existing_teb, new_teb] {
        let vector = v.mem.read_u32(teb + 0x2c).unwrap();
        let block = v.mem.read_u32(vector + index * 4).unwrap();
        assert_ne!(block, main_block);
        assert_eq!(v.mem.read_u32(block).unwrap(), 37);
        let mut zero = [1; 12];
        v.mem.read_bytes(block + 4, &mut zero).unwrap();
        assert_eq!(zero, [0; 12]);
        let saved_teb = v.cpu.fs_base;
        v.cpu.fs_base = teb;
        assert!(matches!(
            call(&mut v),
            wire::cpu::GuestCallOutcome::Returned(37)
        ));
        v.cpu.fs_base = saved_teb;
    }
    assert_eq!(v.k("FreeLibrary", &[module]), 1);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, main_block]),
        u32::MAX
    );
    for teb in [TEB_ADDR, existing_teb, new_teb] {
        let vector = v.mem.read_u32(teb + 0x2c).unwrap();
        assert_eq!(v.mem.read_u32(vector + index * 4).unwrap(), 0);
    }
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), module);
    assert_eq!(v.mem.read_u32(DATA + 0x720).unwrap(), 37); // fresh template after reload
}

#[test]
fn dll_tls_failure_rolls_back_and_invalid_callback_targets_are_rejected() {
    let mut v = Vm::new();
    std::fs::write(v.dir.join("tls.dll"), test_tls_dll(false)).unwrap();
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 1114);
    assert!(!v.mem.is_committed(0x50000000));
    let vector = v.mem.read_u32(TEB_ADDR + 0x2c).unwrap();
    assert_eq!(v.mem.read_u32(vector).unwrap(), 0);
    let mut callback = test_tls_dll(true);
    callback[0xb30..0xb34].copy_from_slice(&0x10004000u32.to_le_bytes());
    std::fs::write(v.dir.join("tls.dll"), callback).unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 193);
    assert!(!v.mem.is_committed(0x50000000));
    let mut bad = test_tls_dll(true);
    bad[0xa08..0xa0c].copy_from_slice(&0x10004000u32.to_le_bytes());
    std::fs::write(v.dir.join("tls.dll"), bad).unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 193);
    assert!(!v.mem.is_committed(0x50000000));
}

#[test]
fn dll_last_page_file_padding_is_mapped_but_cannot_extend_into_another_page() {
    let mut v = Vm::new();
    let mut bytes = test_dll("kernel32.dll!GetCurrentProcessId", true, 9);
    // Declared virtual extent ends within the last page; aligned file data
    // continues through that page, as in the real MinGW codec extension.
    bytes[0xd0..0xd4].copy_from_slice(&0x2f10u32.to_le_bytes());
    bytes[0x180..0x184].copy_from_slice(&0x1f10u32.to_le_bytes());
    bytes[0x21ff] = 0x5a;
    std::fs::write(v.dir.join("tail.dll"), &bytes).unwrap();
    v.mem.write_bytes(DATA, b"tail.dll\0").unwrap();
    let h = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(h, 0);
    assert_eq!(v.mem.read_u8(h + 0x2fff).unwrap(), 0x5a);
    assert_eq!(
        v.mem.virtual_query(h + 0x2fff).unwrap().protect,
        wire::mem::PAGE_EXECUTE_READWRITE
    );
    assert!(v.mem.read_u8(h + 0x3000).is_err());
    assert_eq!(v.k("FreeLibrary", &[h]), 1);
    bytes.resize(0x2400, 0);
    bytes[0x188..0x18c].copy_from_slice(&0x2200u32.to_le_bytes());
    std::fs::write(v.dir.join("tail.dll"), &bytes).unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 193);
    assert!(!v.mem.is_committed(h));
}

#[test]
fn tls_callbacks_run_before_dllmain_on_the_owner_and_before_storage_is_released() {
    let mut v = Vm::new();
    v.mem.write_u32(TEB_ADDR + 0x18, TEB_ADDR).unwrap();
    let mut bytes = test_tls_dll(true);
    // callback records reason, owner TEB and the still-live TLS value.
    let mut code = vec![0x8b, 0x44, 0x24, 8, 0xff, 0x04, 0x85];
    code.extend((DATA + 0x740).to_le_bytes());
    code.extend([0x64, 0x8b, 0x0d, 0x18, 0, 0, 0, 0x89, 0x0c, 0x85]);
    code.extend((DATA + 0x760).to_le_bytes());
    code.extend([
        0x8b, 0x4c, 0x24, 4, 0x8b, 0x89, 0x20, 0x19, 0, 0, 0x64, 0x8b, 0x15, 0x2c, 0, 0, 0, 0x8b,
        0x0c, 0x8a, 0x8b, 0x11, 0x89, 0x14, 0x85,
    ]);
    code.extend((DATA + 0x780).to_le_bytes());
    code.extend([0xc2, 12, 0]);
    bytes[0x2c0..0x2c0 + code.len()].copy_from_slice(&code);
    bytes[0xb30..0xb34].copy_from_slice(&0x100010c0u32.to_le_bytes());
    // The callback table target itself also needs HIGHLOW relocation.
    bytes[0x904..0x908].copy_from_slice(&24u32.to_le_bytes());
    bytes[0x914..0x918].copy_from_slice(&[0x30, 0x39, 0, 0]);
    bytes[0x124..0x128].copy_from_slice(&24u32.to_le_bytes());
    std::fs::write(v.dir.join("tls.dll"), &bytes).unwrap();
    v.mem.write_bytes(DATA, b"tls.dll\0").unwrap();
    let h = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(h, 0);
    assert_eq!(v.mem.read_u32(DATA + 0x744).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x764).unwrap(), TEB_ADDR);
    assert_eq!(v.mem.read_u32(DATA + 0x784).unwrap(), 37);
    assert_eq!(v.mem.read_u32(DATA + 0x720).unwrap(), 37);
    assert_eq!(v.k("DisableThreadLibraryCalls", &[h]), 0);
    assert_eq!(v.k("GetLastError", &[]), 126);
    // Suspended thread receives no callback until it actually runs.
    let exit = v.api.register("kernel32.dll", Some("ExitThread"), None);
    v.mem.write_bytes(CODE, &[0x6a, 7, 0xb8]).unwrap();
    v.mem.write_u32(CODE + 3, exit).unwrap();
    v.mem.write_bytes(CODE + 7, &[0xff, 0xd0]).unwrap();
    let worker = v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    let teb = v.sched.threads[v.sched.thread_index_by_handle(worker).unwrap()].teb;
    assert_eq!(v.mem.read_u32(DATA + 0x748).unwrap(), 0);
    v.k("ResumeThread", &[worker]);
    v.k("Sleep", &[0]);
    // Owner resumes after the worker exits; stop before it can dispatch a
    // synthetic API frame from Vm::call's return address.
    v.sched.threads[0].ctx.eip = RET;
    v.mem.write_bytes(RET, &[0xeb, 0xfe]).unwrap();
    let outcome = v.cpu.run(
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        10000,
    );
    assert!(matches!(outcome, wire::cpu::RunOutcome::InstructionLimit));
    assert_eq!(v.mem.read_u32(DATA + 0x748).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x74c).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x768).unwrap(), teb);
    assert_eq!(v.mem.read_u32(DATA + 0x76c).unwrap(), teb);
    assert_eq!(v.mem.read_u32(DATA + 0x78c).unwrap(), 37);
    let vector = v.mem.read_u32(teb + 0x2c).unwrap();
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, vector]),
        u32::MAX
    );
    assert_eq!(v.k("FreeLibrary", &[h]), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x740).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x780).unwrap(), 37);
    // A malformed callback list never reaches guest execution.
    bytes[0xb30..0xb34].copy_from_slice(&0x10001900u32.to_le_bytes());
    bytes[0x19c..0x1a0].copy_from_slice(&0xc0000040u32.to_le_bytes());
    std::fs::write(v.dir.join("tls.dll"), &bytes).unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 193);
}

#[test]
fn dll_mapping_relocation_exports_initialization_and_reference_counts() {
    let mut v = Vm::new();
    let heap = v.state.process_heap;
    let allocation = v.k("HeapAlloc", &[heap, 0, 64]);
    v.mem.write_u32(allocation, 0xdead1234).unwrap();
    std::fs::write(
        v.dir.join("demo.dll"),
        test_dll("kernel32.dll!GetCurrentProcessId", true, 0x12345678),
    )
    .unwrap();
    v.wide(DATA, "C:\\demo.dll");
    let handle = v.k("LoadLibraryW", &[DATA]);
    assert_eq!(handle, 0x50000000);
    assert_eq!(v.mem.read_u32(handle + 0x1210).unwrap(), handle + 0x1200);
    assert_eq!(v.mem.read_u32(allocation).unwrap(), 0xdead1234);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x708).unwrap(), handle);
    assert_eq!(v.mem.read_u32(DATA + 0x70c).unwrap(), 4242);
    let function = dll_export(&mut v, handle, "GetValue");
    assert_eq!(function, handle + 0x1080);
    assert_eq!(v.k("GetProcAddress", &[handle, 7]), function);
    assert_eq!(dll_export(&mut v, handle, "Data"), handle + 0x1200);
    assert_eq!(dll_export(&mut v, handle, "getvalue"), 0);
    assert_eq!(dll_export(&mut v, handle, "GetPluginInfo"), 0);
    assert_eq!(v.k("GetLastError", &[]), 127);
    assert_eq!(v.k("GetProcAddress", &[handle, 11]), 0);
    assert_eq!(
        dll_export(&mut v, handle, "Alias"),
        v.api.lookup("kernel32.dll", "GetCurrentProcessId").unwrap()
    );
    assert!(matches!(
        v.cpu
            .call_guest(
                &mut v.mem,
                &mut v.api,
                &mut v.state,
                &mut v.log,
                &mut v.sched,
                function,
                &[],
                1_000_000
            )
            .unwrap(),
        wire::cpu::GuestCallOutcome::Returned(0x12345678)
    ));
    v.mem.write_bytes(DATA, b"DEMO.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), handle);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 1);
    assert_eq!(v.k("GetModuleHandleA", &[DATA]), handle);
    assert_eq!(v.k("GetModuleHandleExA", &[6, function, DATA + 0x300]), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), handle);
    assert_eq!(v.k("GetModuleFileNameW", &[handle, DATA + 0x400, 260]), 11);
    assert_eq!(v.mem.read_wstr(DATA + 0x400, 260).unwrap(), "C:\\demo.dll");
    assert_eq!(v.k("GetModuleFileNameW", &[handle, DATA + 0x400, 4]), 4);
    assert_eq!(v.mem.read_wstr(DATA + 0x400, 4).unwrap(), "C:\\");
    assert_eq!(v.k("FreeLibrary", &[handle]), 1);
    assert!(v.mem.is_committed(handle));
    assert_eq!(v.k("FreeLibrary", &[handle]), 1);
    assert!(!v.mem.is_committed(handle));
    assert_eq!(v.mem.read_u32(DATA + 0x704).unwrap(), 1);
    assert_eq!(v.k("GetModuleHandleA", &[DATA]), 0);
    assert_eq!(v.k("GetProcAddress", &[handle, 7]), 0);
    assert_eq!(v.k("GetLastError", &[]), 6);
    assert_eq!(v.k("LoadLibraryA", &[DATA]), handle);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 2);
    assert_eq!(v.mem.read_u32(handle + 0x1200).unwrap(), 0x12345678);
}

#[test]
fn dll_sidecar_ir_handles_relocation_unload_reload_and_stale_source_hashes() {
    let mut v = Vm::new();
    v.cpu.profile_opcodes = false;
    v.cpu
        .install_ir(wire::ir::Image::from_code(CODE, &[]).unwrap(), &v.mem)
        .unwrap();
    let bytes = test_dll("kernel32.dll!GetCurrentProcessId", true, 0x12345678);
    std::fs::write(v.dir.join("aot.dll"), &bytes).unwrap();
    let image = wire::ir::Image::from_pe(&bytes).unwrap();
    std::fs::write(v.dir.join("aot.dll.wir"), image.encode()).unwrap();
    v.wide(DATA, "aot.dll");
    for i in 0..2 {
        let before = v.cpu.ir_steps;
        let module = v.k("LoadLibraryW", &[DATA]);
        assert_eq!(module, 0x50000000);
        assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), i + 1);
        assert_eq!(v.mem.read_u32(DATA + 0x70c).unwrap(), 4242);
        assert!(
            v.cpu.ir_steps > before,
            "DllMain actually executes module IR"
        );
        let function = dll_export(&mut v, module, "GetValue");
        assert!(matches!(
            v.cpu
                .call_guest(
                    &mut v.mem,
                    &mut v.api,
                    &mut v.state,
                    &mut v.log,
                    &mut v.sched,
                    function,
                    &[],
                    1_000_000
                )
                .unwrap(),
            wire::cpu::GuestCallOutcome::Returned(0x12345678)
        ));
        // The relocated absolute MOV cannot use the original operand.
        assert!(v.cpu.ir_fallbacks > 0);
        assert_eq!(v.k("FreeLibrary", &[module]), 1);
        assert!(!v.mem.is_committed(module));
    }
    // A matching path is insufficient: a changed source must run as x86.
    std::fs::write(
        v.dir.join("aot.dll"),
        test_dll("kernel32.dll!GetCurrentProcessId", true, 0xcafebabe),
    )
    .unwrap();
    let before = v.cpu.ir_steps;
    let module = v.k("LoadLibraryW", &[DATA]);
    assert_ne!(module, 0);
    let function = dll_export(&mut v, module, "GetValue");
    assert!(matches!(
        v.cpu
            .call_guest(
                &mut v.mem,
                &mut v.api,
                &mut v.state,
                &mut v.log,
                &mut v.sched,
                function,
                &[],
                1_000_000
            )
            .unwrap(),
        wire::cpu::GuestCallOutcome::Returned(0xcafebabe)
    ));
    assert_eq!(v.cpu.ir_steps, before);
    assert_eq!(v.k("FreeLibrary", &[module]), 1);
}

#[test]
fn dll_dependencies_initialize_before_parent_and_failure_rolls_back() {
    let mut v = Vm::new();
    std::fs::write(
        v.dir.join("child.dll"),
        test_dll("kernel32.dll!GetCurrentProcessId", true, 0xcafe),
    )
    .unwrap();
    std::fs::write(
        v.dir.join("parent.dll"),
        test_dll("child.dll!GetValue", true, 0xbeef),
    )
    .unwrap();
    v.mem.write_bytes(DATA, b"parent.dll\0").unwrap();
    let parent = v.k("LoadLibraryA", &[DATA]);
    assert_eq!(parent, 0x50000000);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 2);
    assert_eq!(v.mem.read_u32(DATA + 0x70c).unwrap(), 0xcafe);
    assert_eq!(v.mem.read_u32(parent + 0x1400).unwrap(), 0x50011080);
    assert_eq!(v.k("FreeLibrary", &[parent]), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x704).unwrap(), 2);
    assert!(!v.mem.is_committed(0x50010000));

    std::fs::write(
        v.dir.join("bad.dll"),
        test_dll("child.dll!GetValue", false, 0xbeef),
    )
    .unwrap();
    v.mem.write_bytes(DATA, b"bad.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 1114);
    assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 4);
    assert_eq!(v.mem.read_u32(DATA + 0x704).unwrap(), 4); // failed parent and successful child detached
    assert!(!v.mem.is_committed(0x50000000));
    assert!(!v.mem.is_committed(0x50010000));
    assert_eq!(v.k("GetModuleHandleA", &[DATA]), 0);
}

#[test]
fn dll_guest_forwarders_deduplicate_dependencies_and_failed_load_preserves_existing_modules() {
    let mut v = Vm::new();
    let child = test_dll("kernel32.dll!GetCurrentProcessId", true, 73);
    std::fs::write(v.dir.join("child.dll"), &child).unwrap();
    v.mem.write_bytes(DATA, b"child.dll\0").unwrap();
    let child_handle = v.k("LoadLibraryA", &[DATA]);
    std::fs::write(
        v.dir.join("bad.dll"),
        test_dll("child.dll!GetValue", false, 0),
    )
    .unwrap();
    v.mem.write_bytes(DATA, b"bad.dll\0").unwrap();
    assert_eq!(v.k("LoadLibraryA", &[DATA]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x704).unwrap(), 1); // failed parent detached, existing child kept
    assert!(v.mem.is_committed(child_handle));
    assert_eq!(v.k("FreeLibrary", &[child_handle]), 1); // failed transaction released its ref
    assert!(!v.mem.is_committed(child_handle));

    let alias = |forward: &str| {
        let mut bytes = test_dll("kernel32.dll!GetCurrentProcessId", true, 81);
        bytes[0x7e0..0x800].fill(0);
        bytes[0x7e0..0x7e0 + forward.len()].copy_from_slice(forward.as_bytes());
        bytes
    };
    std::fs::write(v.dir.join("alias.dll"), alias("child.#7")).unwrap();
    v.mem.write_bytes(DATA, b"alias.dll\0").unwrap();
    let parent = v.k("LoadLibraryA", &[DATA]);
    for _ in 0..3 {
        assert_eq!(dll_export(&mut v, parent, "Alias"), 0x50011080);
    }
    assert_eq!(v.k("FreeLibrary", &[parent]), 1);
    assert!(!v.mem.is_committed(0x50010000));
    std::fs::write(v.dir.join("a.dll"), alias("b.Alias")).unwrap();
    std::fs::write(v.dir.join("b.dll"), alias("a.Alias")).unwrap();
    v.mem.write_bytes(DATA, b"a.dll\0").unwrap();
    let a = v.k("LoadLibraryA", &[DATA]);
    assert_eq!(dll_export(&mut v, a, "Alias"), 0);
    assert_eq!(v.k("GetLastError", &[]), 127);
    assert!(!v.mem.is_committed(0x50010000));
    assert!(v.mem.is_committed(a));
    assert_eq!(v.k("FreeLibrary", &[a]), 1);
    assert!(!v.mem.is_committed(a));
}

#[test]
fn dll_qualified_module_names_and_native_stub_addresses_resolve_correctly() {
    let mut v = Vm::new();
    let bytes = test_dll("kernel32.dll!GetCurrentProcessId", true, 9);
    for dir in ["one", "two"] {
        std::fs::create_dir(v.dir.join(dir)).unwrap();
        std::fs::write(v.dir.join(dir).join("same.dll"), &bytes).unwrap();
    }
    v.wide(DATA, "C:\\one\\same.dll");
    let one = v.k("LoadLibraryW", &[DATA]);
    v.wide(DATA, "C:\\two\\same.dll");
    let two = v.k("LoadLibraryW", &[DATA]);
    assert_ne!(one, two);
    assert_eq!(v.k("GetModuleHandleW", &[DATA]), two);
    v.wide(DATA, "C:\\one\\same.dll");
    assert_eq!(v.k("GetModuleHandleW", &[DATA]), one);
    v.wide(DATA, "C:\\absent\\same.dll");
    assert_eq!(v.k("GetModuleHandleW", &[DATA]), 0);
    let stub = v.api.lookup("kernel32.dll", "GetCurrentProcessId").unwrap();
    assert_eq!(v.k("GetModuleHandleExW", &[6, stub, DATA + 0x300]), 1);
    assert_eq!(
        v.mem.read_u32(DATA + 0x300).unwrap(),
        0x7000_1000 // First persistent PE32 provider; keep its legacy handle.
    );
}

#[test]
fn main_image_registers_real_exports_and_honors_pe_stack_reserve() {
    let mut v = Vm::new();
    let mut exe = test_dll("kernel32.dll!GetCurrentProcessId", true, 19);
    exe[0x96..0x98].copy_from_slice(&0x102u16.to_le_bytes());
    exe[0xb4..0xb8].copy_from_slice(&0x400000u32.to_le_bytes());
    exe[0xe0..0xe4].copy_from_slice(&0x100000u32.to_le_bytes());
    let path = v.dir.join("test.exe");
    std::fs::write(&path, &exe).unwrap();
    let mut mem = GuestMem::new().unwrap();
    let mut table = ApiTable::new();
    let loaded = wire::loader::load_image(&mut mem, &mut table, &path, "test.exe").unwrap();
    let top = STACK_BASE + STACK_SIZE;
    assert_eq!(mem.read_u32(TEB_ADDR + 4).unwrap(), top);
    assert_eq!(mem.read_u32(TEB_ADDR + 8).unwrap(), top - 0x100000);
    assert!(mem.is_committed(top - 0x100000));
    assert!(!mem.is_committed(top - 0x100000 - 0x10000));
    assert!(mem.read_u8(0x17f000).is_ok()); // actual CRT stack probe previously faulted here
    assert_eq!(
        mem.virtual_query(0x400000).unwrap().kind,
        wire::mem::MEM_IMAGE
    );
    assert_eq!(
        mem.virtual_query(0x400000).unwrap().protect,
        wire::mem::PAGE_READONLY
    );
    assert_eq!(
        mem.virtual_query(0x401000).unwrap().protect,
        wire::mem::PAGE_EXECUTE_READWRITE
    );
    assert_eq!(
        mem.virtual_query(top - 4096).unwrap().protect,
        wire::mem::PAGE_READWRITE
    );
    assert!(mem.write_u32(0x400000, 0).is_err());
    v.mem = mem;
    v.api = table;
    v.mem.commit(DATA, 0x10000).unwrap();
    v.state
        .modules
        .register_main(
            &v.mem,
            loaded.image_base,
            path.clone(),
            "C:\\test.exe".into(),
        )
        .unwrap();
    assert_eq!(v.k("GetModuleHandleA", &[0]), 0x400000);
    assert_eq!(dll_export(&mut v, 0x400000, "GetValue"), 0x401080);
    assert_eq!(dll_export(&mut v, 0x400000, "Data"), 0x401200);
    assert_eq!(v.k("FreeLibrary", &[0x400000]), 1);
    assert!(v.mem.is_committed(0x400000));

    exe[0xe0..0xe4].copy_from_slice(&0x200000u32.to_le_bytes());
    std::fs::write(&path, &exe).unwrap();
    let mut mem = GuestMem::new().unwrap();
    assert!(
        wire::loader::load_image(&mut mem, &mut v.api, &path, "test.exe")
            .err()
            .unwrap()
            .contains("stack reserve")
    );
}

#[test]
fn dll_missing_imports_and_malformed_images_fail_without_fake_modules() {
    let mut v = Vm::new();
    let valid = test_dll("kernel32.dll!GetCurrentProcessId", true, 123);
    let mut cases = vec![
        (test_dll("missing.dll!Absent", true, 0), 126),
        (test_dll("kernel32.dll!MadeUpExport", true, 0), 127),
    ];
    let mut no_reloc = valid.clone();
    no_reloc[0x120..0x128].fill(0);
    no_reloc[0x96..0x98].copy_from_slice(&0x2103u16.to_le_bytes()); // fixed image, relocations stripped
    cases.push((no_reloc, 193));
    let mut wrong_reloc = valid.clone();
    wrong_reloc[0x908..0x90a].copy_from_slice(&0x7001u16.to_le_bytes());
    cases.push((wrong_reloc, 193));
    let mut bad_export = valid.clone();
    bad_export[0x740..0x744].copy_from_slice(&0x4000u32.to_le_bytes());
    cases.push((bad_export, 193));
    let mut bad_section = valid.clone();
    bad_section[0x188..0x18c].copy_from_slice(&0x4000u32.to_le_bytes());
    cases.push((bad_section, 193));
    for (index, (bytes, error)) in cases.into_iter().enumerate() {
        let name = format!("bad{index}.dll");
        std::fs::write(v.dir.join(&name), bytes).unwrap();
        v.mem
            .write_bytes(DATA, format!("{name}\0").as_bytes())
            .unwrap();
        assert_eq!(v.k("LoadLibraryA", &[DATA]), 0, "{name}");
        assert_eq!(v.k("GetLastError", &[]), error, "{name}");
        assert_eq!(v.k("GetModuleHandleA", &[DATA]), 0);
        assert!(!v.mem.is_committed(0x50000000));
        assert_eq!(v.mem.read_u32(DATA + 0x700).unwrap(), 0);
    }
    assert_eq!(v.k("LoadLibraryExA", &[DATA, 0, 2]), 0);
    assert_eq!(v.k("GetLastError", &[]), 87);
}

#[test]
fn resources_load_real_utf16_strings_and_bound_lengths() {
    let mut v = Vm::new();
    let base = v.state.exe_base;
    v.mem.commit(base, 0x10000).unwrap();
    v.mem.write_u32(base + 0x3c, 0x80).unwrap();
    v.mem.write_u32(base + 0x80, 0x4550).unwrap();
    v.mem.write_u32(base + 0x80 + 24 + 112, 0x1000).unwrap();
    v.mem.write_u32(base + 0x80 + 24 + 116, 0x1000).unwrap();
    let root = base + 0x1000;
    for off in [0, 24, 48] {
        v.mem.write_u16(root + off + 14, 1).unwrap();
    }
    for (off, value) in [
        (16, 6),
        (20, 0x80000018),
        (40, 1),
        (44, 0x80000030),
        (64, 0x411),
        (68, 72),
        (72, 0x2000),
        (76, 36),
    ] {
        v.mem.write_u32(root + off, value).unwrap();
    }
    v.mem.write_u16(base + 0x2000, 2).unwrap();
    v.wide(base + 0x2002, "日本");
    assert_eq!(v.call("user32.dll", "LoadStringW", &[base, 0, DATA, 10]), 2);
    assert_eq!(v.mem.read_wstr(DATA, 10).unwrap(), "日本");
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 236); // ret + four stdcall args
    assert_eq!(v.call("user32.dll", "LoadStringW", &[base, 0, DATA, 0]), 2);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), base + 0x2002);
    assert_eq!(v.call("user32.dll", "LoadStringA", &[base, 0, DATA, 10]), 4);
    let mut b = [0; 5];
    v.mem.read_bytes(DATA, &mut b).unwrap();
    assert_eq!(b, [0x93, 0xfa, 0x96, 0x7b, 0]);
    let res = v.k("FindResourceW", &[base, 1, 6]);
    assert_eq!(res, root + 72);
    assert_eq!(v.k("SizeofResource", &[base, res]), 36);
    assert_eq!(v.k("LoadResource", &[base, res]), base + 0x2000);
    v.mem.write_u16(base + 0x2000, 0xffff).unwrap();
    assert_eq!(v.call("user32.dll", "LoadStringW", &[base, 0, DATA, 10]), 0);
    assert_eq!(v.mem.read_u16(DATA).unwrap(), 0);
}
