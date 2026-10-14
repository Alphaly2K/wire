//! Existing behavioral tests grouped by files; expectations remain independent of API catalogs.

use super::*;

#[test]
fn mutation_contract_baseline_keeps_win64_flag_width_and_path_fault_order() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let root =
        std::env::temp_dir().join(format!("wire64-mutation-contract-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("source.bin"), b"original").unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let source = STACK + 0x100;
    let destination = STACK + 0x200;
    let frame = STACK + 0x1f88;
    let high = 0xface0000_00000000;
    mem.write(source, b"C:\\source.bin\0").unwrap();
    mem.write(destination, b"C:\\target.bin\0").unwrap();
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetLastError", &[0x55]);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "kernelbase.dll",
            "MoveFileExA",
            &[source, destination, high],
        ),
        1
    );
    assert!(!root.join("source.bin").exists());
    assert_eq!(std::fs::read(root.join("target.bin")).unwrap(), b"original");
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0x55
    );
    // Guest path decoding precedes rejecting unsupported flags.
    cpu.regs[4] = frame;
    mem.write_int(frame, 8, BASE).unwrap();
    cpu.regs[1] = 0xdead0000;
    cpu.regs[2] = destination;
    cpu.regs[8] = 4;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("kernelbase.dll", "MoveFileExA", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "kernelbase.dll",
            "MoveFileExA",
            &[destination, 0, high],
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        50
    );
    assert_eq!(std::fs::read(root.join("target.bin")).unwrap(), b"original");
    for symbol in ["CopyFileA", "CopyFileW"] {
        for dll in ["kernel32.dll", "kernelbase.dll"] {
            assert!(!wire::x64::runtime::import_supported(dll, symbol));
        }
    }
    assert!(!wire::x64::runtime::import_supported(
        "user32.dll",
        "MoveFileExA"
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn metadata_contract_baseline_keeps_win64_widths_error_order_and_return_slot_alias() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let root =
        std::env::temp_dir().join(format!("wire64-metadata-contract-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("info.bin"), b"abc").unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let name = STACK + 0x100;
    let output = STACK + 0x200;
    let frame = STACK + 0x1f88;
    let high = 0xface0000_00000000;
    mem.write(name, b"C:\\info.bin\0").unwrap();
    let handle = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[name, 0x80000000, 7, 0, 3, 0x80, 0],
    );
    assert_ne!(handle, u64::MAX);
    // Invalid class precedes decoding the bad path and checking the bad output.
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "kernelbase.dll",
            "GetFileAttributesExA",
            &[0xdead0000, high | 1, 0xdead0000],
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        87
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetLastError", &[0x55]);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "kernelbase.dll",
            "GetFileAttributesExA",
            &[name, high, frame],
        ),
        1
    );
    assert_eq!(mem.read_int(frame, 4).unwrap(), 0x80);
    assert_eq!(cpu.rip, BASE);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0x55
    );
    mem.write(output, &[0xa5; 64]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "kernelbase.dll",
            "GetFileInformationByHandleEx",
            &[handle, high | 9, output, high | 7],
        ),
        0
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        24
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "kernelbase.dll",
            "GetFileInformationByHandleEx",
            &[handle, high | 9, frame, high | 8],
        ),
        1
    );
    assert_eq!(mem.read_int(frame, 8).unwrap(), 0x80);
    assert_eq!(cpu.rip, BASE);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "kernelbase.dll",
            "GetFileTime",
            &[handle, frame, 0, 0],
        ),
        1
    );
    assert!(mem.read_int(frame, 8).unwrap() > 116444736000000000);
    assert_eq!(cpu.rip, BASE);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileInformationByHandle",
            &[u64::MAX, 0xdead0000],
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        6
    );
    for symbol in [
        "GetFileAttributesExA",
        "GetFileAttributesExW",
        "GetFileInformationByHandle",
        "GetFileInformationByHandleEx",
        "GetFileTime",
    ] {
        assert!(!wire::x64::runtime::import_supported("user32.dll", symbol));
    }
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[handle]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn path_contract_baseline_keeps_win64_capacity_alias_and_absent_exports() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let root = std::env::temp_dir().join(format!("wire64-path-contract-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let high = 0xabcd0000_00000000;
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetLastError", &[0x55]);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "kernelbase.dll",
            "GetCurrentDirectoryA",
            &[high, 0xdead0000]
        ),
        4
    );
    let frame = STACK + 0x1f88;
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "kernelbase.dll",
            "GetCurrentDirectoryA",
            &[high | 4, frame]
        ),
        3
    );
    let mut bytes = [0; 4];
    mem.read(frame, &mut bytes).unwrap();
    assert_eq!(&bytes, b"C:\\\0");
    assert_eq!(cpu.rip, BASE);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0x55
    );
    assert!(!wire::x64::runtime::import_supported(
        "user32.dll",
        "GetCurrentDirectoryA"
    ));
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        for name in ["GetFullPathNameA", "GetFullPathNameW"] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn core_file_contract_keeps_win64_widths_preflight_and_return_slot_alias() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let root = std::env::temp_dir().join(format!("wire64-core-contract-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("read.bin"), b"abc").unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let path = STACK + 0x100;
    let buffer = STACK + 0x200;
    let output = STACK + 0x300;
    let high = 0xabcd0000_00000000;
    mem.write(path, b"C:\\read.bin\0").unwrap();
    let file = startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "kernelbase.dll",
        "CreateFileA",
        &[
            path,
            high | 0x80000000,
            high | 3,
            0,
            high | 3,
            high | 0x80,
            0,
        ],
    );
    assert_ne!(file, u64::MAX);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetFilePointerEx",
            &[file, 0x1_0000_0002, output, high]
        ),
        1
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 0x1_0000_0002);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetFilePointerEx",
            &[file, (-0x1_0000_0002i64) as u64, output, high | 1]
        ),
        1
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 0);
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = file;
    cpu.regs[2] = 2;
    cpu.regs[8] = STACK + 8192 - 4;
    cpu.regs[9] = 0;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("kernel32.dll", "SetFilePointerEx", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(runtime.api_calls, calls);
    // Win64 checks the seek output before changing the file offset.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[file, buffer, high | 3, STACK + 0x1f88, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(STACK + 0x1f88, 4).unwrap(), 3);
    let mut bytes = [0; 3];
    mem.read(buffer, &mut bytes).unwrap();
    assert_eq!(&bytes, b"abc");
    assert!(!wire::x64::runtime::import_supported(
        "user32.dll",
        "ReadFile"
    ));
    assert!(!wire::x64::runtime::import_supported(
        "kernel32.dll",
        "SetEndOfFile"
    ));
    assert!(!wire::x64::runtime::import_supported(
        "kernel32.dll",
        "SetFilePointer"
    ));
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[file]),
        1
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn win64_standard_handle_redirects_aliases_and_faults_keep_endpoint_lifetimes() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let out = STACK + 0x100;
    let flags = out + 16;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetStdHandle",
            &[0xfffffff5]
        ),
        0x101
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetStdHandle",
            &[0xfffffff5, 0x102]
        ),
        1
    );
    // Changing the process redirect table does not close the original endpoint.
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetFileType", &[0x101]),
        3
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetHandleInformation",
            &[0x101, 2, 2]
        ),
        1
    );
    mem.write_int(out, 8, 0xdeadbeef).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, 0x101, u64::MAX, out, 0, 1, 3]
        ),
        0
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xdeadbeef);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        6
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetHandleInformation",
            &[0x101, 2, 0]
        ),
        1
    );

    // An invalid output cannot allocate an alias or apply CLOSE_SOURCE.
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = u64::MAX;
    cpu.regs[2] = 0x101;
    cpu.regs[8] = u64::MAX;
    cpu.regs[9] = STACK + 8192 - 4;
    for (i, value) in [0, 1, 3].into_iter().enumerate() {
        mem.write_int(cpu.regs[4] + 40 + i as u64 * 8, 8, value)
            .unwrap();
    }
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "DuplicateHandle", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetFileType", &[0x101]),
        3
    );

    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, 0x101, u64::MAX, out, 0, 1, 3]
        ),
        1
    );
    let alias = mem.read_int(out, 8).unwrap();
    assert_eq!(alias, 0x7fff3000);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetFileType", &[0x101]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetFileType", &[alias]),
        3
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetStdHandle",
            &[0xfffffff5]
        ),
        0x102
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetHandleInformation",
            &[alias, 2, 2]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, alias, u64::MAX, out, 0, 0, 2]
        ),
        1
    );
    let second = mem.read_int(out, 8).unwrap();
    assert_eq!(second, 0x7fff3001);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[alias, flags]
        ),
        1
    );
    assert_eq!(mem.read_int(flags, 4).unwrap(), 3);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[second, flags]
        ),
        1
    );
    assert_eq!(mem.read_int(flags, 4).unwrap(), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetStdHandle",
            &[0xfffffff5, second]
        ),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[second]),
        1
    );
    // GetStdHandle returns the stored value even when that endpoint was closed.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetStdHandle",
            &[0xfffffff5]
        ),
        second
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetFileType", &[second]),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetHandleInformation",
            &[alias, 2, 0]
        ),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]),
        1
    );
}

#[test]
fn win64_create_directory_validates_default_security_and_faults_before_creating() {
    let (mut mem, mut cpu, mut runtime) = startup();
    const DATA: u64 = 0x6e0000000;
    mem.map(DATA, 4096, READ | WRITE).unwrap();
    let root = std::env::temp_dir().join(format!("wire-mkdir64-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let security = DATA + 256;
    for (name, api, inherit) in [
        ("日本", "CreateDirectoryW", 1),
        ("plain", "CreateDirectoryA", 0),
    ] {
        let path = format!("\\\\?\\C:\\{name}\0");
        let bytes: Vec<u8> = if api.ends_with('W') {
            path.encode_utf16().flat_map(u16::to_le_bytes).collect()
        } else {
            path.into_bytes()
        };
        mem.write(DATA, &bytes).unwrap();
        mem.write(security, &[0; 24]).unwrap();
        mem.write_int(security, 4, 24).unwrap();
        mem.write_int(security + 16, 4, inherit).unwrap();
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, api, &[DATA, security]),
            1
        );
        assert!(root.join(name).is_dir());
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, api, &[DATA, security]),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            183
        );
    }
    for length in [0, 12, 24, u32::MAX] {
        let name = format!("length-{length}");
        mem.write(DATA, format!("C:\\{name}\0").as_bytes()).unwrap();
        mem.write_int(security, 4, length as u64).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "CreateDirectoryA",
                &[DATA, security]
            ),
            1
        );
        assert!(root.join(name).is_dir());
    }
    mem.write(DATA, b"C:\\not-created\0").unwrap();
    for (length, descriptor, error) in [(0, 0x123456780000, 50), (24, 0x123456780000, 50)] {
        mem.write_int(security, 4, length).unwrap();
        mem.write_int(security + 8, 8, descriptor).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "CreateDirectoryA",
                &[DATA, security]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            error
        );
        assert!(!root.join("not-created").exists());
    }
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = DATA;
    cpu.regs[2] = DATA + 4096 - 12;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("kernel32.dll", "CreateDirectoryA", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    assert!(!root.join("not-created").exists());
    // Only the eight-byte descriptor field is consumed, not unused tail bytes.
    mem.write(DATA, b"C:\\short-security\0").unwrap();
    mem.write_int(DATA + 4096 - 8, 8, 0).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateDirectoryA",
            &[DATA, DATA + 4096 - 16]
        ),
        1
    );
    assert!(root.join("short-security").is_dir());
    mem.write(DATA, b"C:\\..\\outside\0").unwrap();
    mem.write(security, &[0; 24]).unwrap();
    mem.write_int(security, 4, 24).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateDirectoryA",
            &[DATA, security]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        5
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn win64_directory_handles_share_native_batches_restart_after_rename_and_stay_confined() {
    let (mut mem, mut cpu, mut runtime) = startup();
    const DATA: u64 = 0x6c0000000;
    mem.map(DATA, 8192, READ | WRITE).unwrap();
    let root = std::env::temp_dir().join(format!("wire-directory64-{}", std::process::id()));
    std::fs::create_dir_all(root.join("items/sub")).unwrap();
    std::fs::write(root.join("items/A.txt"), b"abc").unwrap();
    std::fs::File::create(root.join("items/B.txt"))
        .unwrap()
        .set_len(0x100000007)
        .unwrap();
    std::fs::write(root.join("items/あ.txt"), b"x").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(std::env::temp_dir(), root.join("items/escape.txt")).unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    mem.write(DATA, b"C:\\items\0").unwrap();
    let handle = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[DATA, 1, 7, 0, 3, 0x02000000, 0],
    );
    assert_ne!(handle, u64::MAX);
    let iosb = DATA + 128;
    let output = DATA + 256;
    let name = |mem: &Memory, at: u64| {
        let n = mem.read_int(at + 60, 4).unwrap() as usize;
        let mut bytes = vec![0; n];
        mem.read(at + 68, &mut bytes).unwrap();
        String::from_utf16(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect::<Vec<_>>(),
        )
        .unwrap()
    };
    let query = |mem: &mut Memory,
                 cpu: &mut Cpu64,
                 runtime: &mut wire::x64::runtime::Runtime64,
                 h,
                 single,
                 restart| {
        startup_dll_call(
            mem,
            cpu,
            runtime,
            "ntdll.dll",
            "NtQueryDirectoryFile",
            &[
                h,
                0,
                0,
                0,
                iosb,
                output,
                4096,
                0x100000002,
                single,
                0,
                restart,
            ],
        )
    };
    assert!(wire::x64::runtime::import_supported(
        "ntdll.dll",
        "NtQueryDirectoryFile"
    ));
    assert!(!wire::x64::runtime::import_supported(
        "kernel32.dll",
        "NtQueryDirectoryFile"
    ));
    runtime.register_provider("ntdll.dll");
    for (dll, expected) in [("ntdll.dll", true), ("kernel32.dll", false)] {
        mem.write(DATA + 6144, format!("{dll}\0").as_bytes())
            .unwrap();
        let module = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "LoadLibraryA",
            &[DATA + 6144],
        );
        mem.write(DATA + 6144, b"NtQueryDirectoryFile\0").unwrap();
        let gateway = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[module, DATA + 6144],
        );
        if expected {
            assert_ne!(gateway, 0);
            assert_eq!(runtime.gateways[&gateway], "ntdll.dll!NtQueryDirectoryFile");
        } else {
            assert_eq!(gateway, 0);
        }
    }
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetLastError", &[0x1234]);
    assert_eq!(query(&mut mem, &mut cpu, &mut runtime, handle, 1, 0), 0);
    assert_eq!(mem.read_int(iosb, 8).unwrap(), 0);
    assert_eq!(mem.read_int(iosb + 8, 8).unwrap(), 80);
    assert_eq!(name(&mem, output), "A.txt");
    assert_eq!(mem.read_int(output + 40, 8).unwrap(), 3);
    let modified = root
        .join("items/A.txt")
        .metadata()
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    assert_eq!(
        mem.read_int(output + 24, 8).unwrap(),
        116444736000000000 + (modified.as_nanos() / 100) as u64
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0x1234
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, handle, u64::MAX, DATA + 96, 0, 0, 2]
        ),
        1
    );
    let copy = mem.read_int(DATA + 96, 8).unwrap();
    assert_eq!(
        query(&mut mem, &mut cpu, &mut runtime, copy, 0x100000001, 0),
        0
    );
    assert_eq!(name(&mem, output), "B.txt");
    assert_eq!(mem.read_int(output + 40, 8).unwrap(), 0x100000007);
    assert_eq!(query(&mut mem, &mut cpu, &mut runtime, handle, 0, 0), 0);
    assert_eq!(name(&mem, output), "sub");
    assert_eq!(mem.read_int(output + 56, 4).unwrap() & 0x10, 0x10);
    assert_eq!(mem.read_int(output + 40, 8).unwrap(), 0);
    let next = mem.read_int(output, 4).unwrap();
    assert_eq!(next % 8, 0);
    assert_eq!(name(&mem, output + next), "あ.txt");
    assert_eq!(mem.read_int(output + next, 4).unwrap(), 0);
    mem.write(output, &[0xcc; 128]).unwrap();
    assert_eq!(
        query(&mut mem, &mut cpu, &mut runtime, copy, 0, 0),
        0x80000006
    );
    assert_eq!(mem.read_int(iosb, 4).unwrap(), 0x80000006);
    assert_eq!(mem.read_int(iosb + 8, 8).unwrap(), 0);
    assert_eq!(mem.read_int(output, 8).unwrap(), 0xcccccccccccccccc);
    std::fs::rename(root.join("items"), root.join("renamed")).unwrap();
    std::fs::create_dir_all(root.join("items")).unwrap();
    std::fs::write(root.join("items/imposter.txt"), b"wrong directory").unwrap();
    assert_eq!(query(&mut mem, &mut cpu, &mut runtime, handle, 1, 1), 0);
    assert_eq!(name(&mem, output), "A.txt");
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[handle]),
        1
    );
    // The duplicate keeps both the real directory descriptor and its cursor alive.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileInformationByHandleEx",
            &[copy, 14, output, 4096]
        ),
        1
    );
    assert_eq!(name(&mem, output), "B.txt");
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileInformationByHandleEx",
            &[copy, 15, output, 4096]
        ),
        1
    );
    assert_eq!(name(&mem, output), "A.txt");
    assert_eq!(
        query(&mut mem, &mut cpu, &mut runtime, copy, 0, 0),
        0x80000006
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[copy]),
        1
    );
    assert_eq!(
        query(&mut mem, &mut cpu, &mut runtime, copy, 0, 0),
        0xc0000008
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn win64_directory_queries_preserve_outputs_and_cursor_on_checked_faults_and_short_buffers() {
    let (mut mem, mut cpu, mut runtime) = startup();
    const DATA: u64 = 0x6d0000000;
    mem.map(DATA, 8192, READ | WRITE).unwrap();
    let root = std::env::temp_dir().join(format!("wire-directory-fault64-{}", std::process::id()));
    std::fs::create_dir_all(root.join("items")).unwrap();
    std::fs::write(root.join("items/A.txt"), b"abc").unwrap();
    std::fs::write(root.join("items/B.bin"), b"x").unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    mem.write(DATA, b"C:\\items\0").unwrap();
    for (access, disposition, flags, error) in [
        (1, 3, 0, 5),
        (2, 3, 0x02000000, 5),
        (1, 2, 0x02000000, 5),
        (1, 3, 0x02200000, 50),
    ] {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "CreateFileA",
                &[DATA, access, 7, 0, disposition, flags, 0]
            ),
            u64::MAX
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            error
        );
    }
    let h = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[DATA, 1, 7, 0, 3, 0x02000000, 0],
    );
    assert_ne!(h, u64::MAX);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateFileA",
            &[DATA, 1, 0, 0, 3, 0x02000000, 0]
        ),
        u64::MAX
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        32
    );
    let iosb = DATA + 128;
    let output = DATA + 256;
    mem.write(iosb, &[0xcc; 16]).unwrap();
    mem.write(output, &[0xcc; 128]).unwrap();
    let mut args = [h, 0, 0, 0, iosb, output, 64, 2, 1, 0, 0];
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "NtQueryDirectoryFile",
            &args
        ),
        0xc0000023
    );
    assert_eq!(mem.read_int(iosb + 8, 8).unwrap(), 0);
    assert_eq!(mem.read_int(output, 8).unwrap(), 0xcccccccccccccccc);
    // Faulting either output must not initialize or advance the directory cursor.
    for (index, address) in [(4, DATA + 8192 - 8), (5, DATA + 8192 - 40)] {
        args[6] = 128;
        args[index] = address;
        mem.write(iosb, &[0xcc; 16]).unwrap();
        cpu.regs[4] = STACK + 0x1f88;
        mem.write_int(cpu.regs[4], 8, BASE).unwrap();
        for (i, &a) in args.iter().enumerate() {
            if i < 4 {
                cpu.regs[[1, 2, 8, 9][i]] = a;
            } else {
                mem.write_int(cpu.regs[4] + 40 + (i as u64 - 4) * 8, 8, a)
                    .unwrap();
            }
        }
        let before = cpu.clone();
        let calls = runtime.api_calls;
        assert!(runtime
            .dispatch("ntdll.dll", "NtQueryDirectoryFile", &mut cpu, &mut mem)
            .is_err());
        assert_eq!(cpu, before);
        assert_eq!(runtime.api_calls, calls);
        assert_eq!(mem.read_int(iosb, 8).unwrap(), 0xcccccccccccccccc);
        assert_eq!(mem.read_int(output, 8).unwrap(), 0xcccccccccccccccc);
        args[4] = iosb;
        args[5] = output;
    }
    // Capture a counted UTF-16 pattern on first success, then ignore later patterns.
    mem.write(DATA + 1024, &[0; 16]).unwrap();
    mem.write_int(DATA + 1024, 2, 10).unwrap();
    mem.write_int(DATA + 1026, 2, 10).unwrap();
    mem.write_int(DATA + 1032, 8, DATA + 1056).unwrap();
    mem.write(
        DATA + 1056,
        &"*.txt"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    args[9] = DATA + 1024;
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "NtQueryDirectoryFile",
            &args
        ),
        0
    );
    assert_eq!(mem.read_int(output + 68, 2).unwrap(), b'A' as u64);
    mem.write(
        DATA + 1056,
        &"*.bin"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    args[10] = 1;
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "NtQueryDirectoryFile",
            &args
        ),
        0
    );
    assert_eq!(mem.read_int(output + 68, 2).unwrap(), b'A' as u64);
    args[10] = 0;
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "NtQueryDirectoryFile",
            &args
        ),
        0x80000006
    );
    args[7] = 4; // FileBasicInformation is not a directory enumeration class.
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "NtQueryDirectoryFile",
            &args
        ),
        0xc0000003
    );
    assert_eq!(mem.read_int(iosb, 4).unwrap(), 0xc0000003);
    args[7] = 2;
    args[1] = 1;
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "NtQueryDirectoryFile",
            &args
        ),
        0xc00000bb
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[h]),
        1
    );
    let metadata_only = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[DATA, 0, 7, 0, 3, 0x02000000, 0],
    );
    args[0] = metadata_only;
    args[1] = 0;
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "NtQueryDirectoryFile",
            &args
        ),
        0xc0000022
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CloseHandle",
            &[metadata_only]
        ),
        1
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn win64_directory_searches_report_real_wide_metadata_and_do_not_skip_entries_after_output_faults()
{
    use wire::x64::process::Process64;
    const DATA: u64 = 0x6b0000000;
    let dir = std::env::temp_dir().join(format!("wire-find64-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("items")).unwrap();
    std::fs::write(dir.join("main.exe"), fixture()).unwrap();
    std::fs::write(dir.join("items/A.txt"), b"abc").unwrap();
    let huge = std::fs::File::create(dir.join("items/B.txt")).unwrap();
    huge.set_len(0x100000007).unwrap();
    std::fs::write(dir.join("items/あ.txt"), b"x").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(std::env::temp_dir(), dir.join("items/escape.txt")).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    p.memory.map(DATA, 8192, READ | WRITE).unwrap();
    p.memory
        .write(
            DATA,
            &"C:\\items\\*.TXT\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let output = DATA + 0x200;
    let handle = startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "FindFirstFileExW",
        &[DATA, 1, output, 0, 0, 2],
    );
    assert!(handle > u32::MAX as u64 && handle != u64::MAX);
    assert_eq!(p.memory.read_int(output + 32, 4).unwrap(), 3);
    assert_eq!(p.memory.read_int(output + 44, 2).unwrap(), b'A' as u64);
    assert_ne!(p.memory.read_int(output + 20, 8).unwrap(), 0);
    let bad = DATA + 8192 - 100;
    p.memory.write(bad, &[0xcc; 100]).unwrap();
    p.cpu.regs[4] = STACK + 0x1f88;
    p.memory.write_int(p.cpu.regs[4], 8, BASE).unwrap();
    p.cpu.regs[1] = handle;
    p.cpu.regs[2] = bad;
    let before = p.cpu.clone();
    let calls = p.runtime.api_calls;
    assert!(p
        .runtime
        .dispatch("kernel32.dll", "FindNextFileW", &mut p.cpu, &mut p.memory)
        .is_err());
    assert_eq!(p.cpu.regs, before.regs);
    assert_eq!(p.cpu.rip, before.rip);
    assert_eq!(p.runtime.api_calls, calls);
    assert_eq!(p.memory.read_int(bad, 8).unwrap(), 0xcccccccccccccccc);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FindNextFileW",
            &[handle, output]
        ),
        1
    );
    assert_eq!(p.memory.read_int(output + 44, 2).unwrap(), b'B' as u64);
    assert_eq!(p.memory.read_int(output + 28, 4).unwrap(), 1);
    assert_eq!(p.memory.read_int(output + 32, 4).unwrap(), 7);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FindNextFileW",
            &[handle, output]
        ),
        1
    );
    assert_eq!(p.memory.read_int(output + 44, 2).unwrap(), 0x3042);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FindNextFileW",
            &[handle, output]
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
        18
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FindClose",
            &[handle]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FindClose",
            &[handle]
        ),
        0
    );
    p.memory.write(DATA, b"C:\\items\\missing*\0").unwrap();
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "FindFirstFileA",
            &[DATA, output]
        ),
        u64::MAX
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn win64_file_move_replace_metadata_and_open_handle_failures_are_confined() {
    let dir = std::env::temp_dir().join(format!("wire-move64-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("source.txt"), b"source").unwrap();
    std::fs::write(dir.join("target.txt"), b"target").unwrap();
    let (mut mem, mut cpu, mut runtime) = startup();
    runtime.set_filesystem(wire::fs::GuestFs::new(&dir).unwrap());
    let source = STACK + 0x100;
    let target = STACK + 0x200;
    mem.write(source, b"source.txt\0").unwrap();
    mem.write(target, b"target.txt\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MoveFileA",
            &[source, target]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        183
    );
    assert_eq!(std::fs::read(dir.join("source.txt")).unwrap(), b"source");
    assert_eq!(std::fs::read(dir.join("target.txt")).unwrap(), b"target");
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetFileAttributesA",
            &[source, 0x2000]
        ),
        1
    );
    let file = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[source, 0x80000000, 7, 0, 3, 0, 0],
    );
    assert_ne!(file, u64::MAX);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MoveFileExA",
            &[source, target, 1]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        32
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[file]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MoveFileExA",
            &[source, target, 1]
        ),
        1
    );
    assert!(!dir.join("source.txt").exists());
    assert_eq!(std::fs::read(dir.join("target.txt")).unwrap(), b"source");
    assert_ne!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileAttributesA",
            &[target]
        ) & 0x2000,
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "DeleteFileA", &[target]),
        1
    );
    assert!(!dir.join("target.txt").exists());
    // Unsupported delayed/cross-volume flags cannot silently move files.
    std::fs::write(dir.join("source.txt"), b"source").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MoveFileExA",
            &[source, target, 4]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        50
    );
    assert!(dir.join("source.txt").is_file());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn win64_anonymous_pipes_transfer_bytes_preserve_data_on_fault_and_report_broken_endpoint() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let pointers = STACK + 0x100;
    let input = STACK + 0x200;
    let output = STACK + 0x300;
    let count = STACK + 0x400;
    let attributes = STACK + 0x500;
    mem.write(attributes, &[0; 24]).unwrap();
    mem.write_int(attributes, 4, 24).unwrap();
    mem.write_int(attributes + 16, 4, 1).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreatePipe",
            &[pointers, pointers + 8, attributes, 0]
        ),
        1
    );
    let reader = mem.read_int(pointers, 8).unwrap();
    let writer = mem.read_int(pointers + 8, 8).unwrap();
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetFileType", &[reader]),
        3
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[reader, count]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 1);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetHandleInformation",
            &[reader, 1, 0]
        ),
        1
    );
    mem.write(input, b"real-stream").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WriteFile",
            &[writer, input, 11, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 11);
    // A bad result pointer must not consume any queued bytes.
    cpu.regs[1] = reader;
    cpu.regs[2] = output;
    cpu.regs[8] = 11;
    cpu.regs[9] = 0;
    mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
    mem.write_int(cpu.regs[4] + 40, 8, 0).unwrap();
    assert!(runtime
        .dispatch("kernel32.dll", "ReadFile", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[reader, output, 4, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 4);
    let mut bytes = [0; 4];
    mem.read(output, &mut bytes).unwrap();
    assert_eq!(&bytes, b"real");
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[writer]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[reader, output, 11, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 7);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[reader, output, 11, count, 0]
        ),
        0
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        109
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[reader]),
        1
    );
}

#[test]
fn win64_filesystem_queries_read_real_metadata_and_keep_cwd_confined() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let root = std::env::temp_dir().join(format!("wire64-files-{}", std::process::id()));
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/test.dat"), b"real-data").unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let path = STACK + 0x100;
    let output = STACK + 0x200;
    mem.write(path, b"C:\\sub\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileAttributesA",
            &[path]
        ) & 0x10,
        0x10
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetCurrentDirectoryA",
            &[path]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetCurrentDirectoryA",
            &[0, 0]
        ),
        7
    );
    mem.write(path, b"test.dat\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileAttributesExA",
            &[path, 0, output]
        ),
        1
    );
    assert_eq!(mem.read_int(output + 32, 4).unwrap(), 9);
    mem.write(path, b"..\\..\\outside\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetCurrentDirectoryA",
            &[path]
        ),
        0
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn win64_readonly_file_sections_keep_views_alive_and_coherent_with_guest_file_writes() {
    use std::io::{Seek, SeekFrom, Write as _};
    let (mut mem, mut cpu, mut runtime) = startup();
    let root = std::env::temp_dir().join(format!("wire64-section-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut file = std::fs::File::create(root.join("section.bin")).unwrap();
    let data: Vec<_> = (0..73729).map(|n| (n % 251) as u8).collect();
    file.write_all(&data).unwrap();
    file.seek(SeekFrom::Start(0x100010000)).unwrap();
    file.write_all(b"high-file-offset").unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let name = STACK + 0x100;
    let out = STACK + 0x300;
    mem.write(name, b"C:\\section.bin\0").unwrap();
    let source = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[name, 0xc0000000, 7, 0, 3, 0x80, 0],
    );
    assert_ne!(source, u64::MAX);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetFilePointerEx",
        &[source, 1234, 0, 0],
    );
    let section = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileMappingW",
        &[source, 0, 2, 0, 0, 0],
    );
    assert_ne!(section, 0);
    const ADDRESS: u64 = 0x550000000;
    let view = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "MapViewOfFileEx",
        &[section, 4, 0, 0, data.len() as u64, ADDRESS],
    );
    assert_eq!(view, ADDRESS);
    let tail = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "MapViewOfFile",
        &[section, 4, 0, 65536, 16],
    );
    assert_ne!(tail, 0);
    let high = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "MapViewOfFile",
        &[section, 4, 1, 65536, 16],
    );
    let mut high_data = [0; 16];
    mem.read(high, &mut high_data).unwrap();
    assert_eq!(&high_data, b"high-file-offset");
    let mut mapped = vec![0; data.len()];
    mem.read(view, &mut mapped).unwrap();
    assert_eq!(mapped, data);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "ReadFile",
        &[source, out, 1, out + 16, 0],
    );
    assert_eq!(mem.read_int(out, 1).unwrap(), data[1234] as u64); // Mapping never seeks the file handle.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualQuery",
            &[view, out, 48]
        ),
        48
    );
    assert_eq!(mem.read_int(out + 8, 8).unwrap(), view);
    assert_eq!(mem.read_int(out + 36, 4).unwrap(), 2);
    assert_eq!(
        mem.read_int(out + 40, 4).unwrap(),
        wire::mem::MEM_MAPPED as u64
    );
    assert!(mem.write_int(view + 4094, 8, 0).is_err());
    mem.write_int(out, 4, 0xa5a5a5a5).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualProtect",
            &[view, 4096, 4, out]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        5
    );
    assert_eq!(mem.read_int(out, 4).unwrap(), 0xa5a5a5a5);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualFree",
            &[view, 0, 0x8000]
        ),
        0
    );
    for (access, offset_hi, offset_lo, size, expected) in [
        (2, 0, 0, 16, 50), // writable view is an explicit unsupported boundary
        (4, 0, 1, 16, 1132),
        (4, 1, 65536, 17, 87),
        (4, 0, 0, 0, 8), // whole >4GiB view exceeds checked memory capacity
    ] {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "MapViewOfFile",
                &[section, access, offset_hi, offset_lo, size]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            expected
        );
    }
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MapViewOfFileEx",
            &[section, 4, 0, 0, 16, ADDRESS]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        487
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, section, u64::MAX, out, 0, 1, 2]
        ),
        1
    );
    let alias = mem.read_int(out, 8).unwrap();
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetHandleInformation",
        &[alias, out],
    );
    assert_eq!(mem.read_int(out, 4).unwrap(), 1);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetHandleInformation",
        &[alias, 2, 2],
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]),
        0
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetHandleInformation",
        &[alias, 2, 0],
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[section]);
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[source]);
    let retained = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "MapViewOfFile",
        &[alias, 4, 0, 0, 16],
    );
    assert_ne!(retained, 0); // descriptor and section survive source closure
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]);
    let writer = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[name, 0x40000000, 7, 0, 3, 0x80, 0],
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetFilePointerEx",
        &[writer, 65532, 0, 0],
    );
    mem.write(out, b"abcdefghijkl").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WriteFile",
            &[writer, out, 12, out + 16, 0]
        ),
        1
    );
    let mut changed = [0; 12];
    mem.read(view + 65532, &mut changed).unwrap();
    assert_eq!(&changed, b"abcdefghijkl");
    assert_eq!(
        mem.read_int(tail, 8).unwrap(),
        u64::from_le_bytes(*b"efghijkl")
    );
    // Backing updates must also preserve PAGE_NOACCESS, until the guest restores READ.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualProtect",
            &[tail, 4096, 1, out]
        ),
        1
    );
    mem.write(out, b"ABCDEFGH").unwrap();
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetFilePointerEx",
        &[writer, 65536, 0, 0],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WriteFile",
            &[writer, out, 8, out + 16, 0]
        ),
        1
    );
    assert!(mem.read_int(tail, 8).is_err());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VirtualProtect",
            &[tail, 4096, 2, out]
        ),
        1
    );
    assert_eq!(
        mem.read_int(tail, 8).unwrap(),
        u64::from_le_bytes(*b"ABCDEFGH")
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateFileA",
            &[name, 0x40000000, 7, 0, 2, 0x80, 0]
        ),
        u64::MAX
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        1224
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "FlushViewOfFile",
            &[view + 65536, 16]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "UnmapViewOfFile",
            &[view + 4096]
        ),
        0
    );
    for base in [view, tail, high, retained] {
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "UnmapViewOfFile", &[base]),
            1
        );
        assert!(mem.read_int(base, 1).is_err());
    }
    let truncated = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[name, 0x40000000, 7, 0, 2, 0x80, 0],
    );
    assert_ne!(truncated, u64::MAX);
    assert_eq!(file.metadata().unwrap().len(), 0);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CloseHandle",
        &[truncated],
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[writer]);
    drop(file);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn win64_handle_metadata_preserves_large_sizes_identity_times_and_checked_outputs() {
    use std::os::unix::fs::MetadataExt;
    let (mut mem, mut cpu, mut runtime) = startup();
    let root = std::env::temp_dir().join(format!("wire64-handle-info-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("large.bin");
    let file = std::fs::File::create(&path).unwrap();
    let length = 0x100000009;
    file.set_len(length).unwrap();
    std::fs::hard_link(&path, root.join("alias.bin")).unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let name = STACK + 0x100;
    let out = STACK + 0x200;
    mem.write(name, b"C:\\large.bin\0").unwrap();
    let handle = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[name, 0x80000000, 7, 0, 3, 0x80, 0],
    );
    assert_ne!(handle, u64::MAX);
    // Rename after open: metadata must come from the live descriptor.
    std::fs::rename(&path, root.join("renamed.bin")).unwrap();
    let meta = file.metadata().unwrap();
    mem.write(out, &[0xa5; 60]).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileInformationByHandle",
            &[handle, out]
        ),
        1
    );
    assert_eq!(mem.read_int(out + 28, 4).unwrap(), meta.dev() as u32 as u64);
    assert_eq!(mem.read_int(out + 32, 4).unwrap(), 1);
    assert_eq!(mem.read_int(out + 36, 4).unwrap(), 9);
    assert_eq!(mem.read_int(out + 40, 4).unwrap(), 2);
    let index = mem.read_int(out + 44, 4).unwrap() << 32 | mem.read_int(out + 48, 4).unwrap();
    assert_eq!(index, meta.ino());
    assert_eq!(mem.read_int(out + 52, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
    let times = [4, 12, 20].map(|offset| mem.read_int(out + offset, 8).unwrap());
    assert!(times.iter().all(|&t| t > 116444736000000000));
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileTime",
            &[handle, out, 0, out + 16]
        ),
        1
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), times[0]);
    assert_eq!(mem.read_int(out + 16, 8).unwrap(), times[2]);
    mem.write(out, &[0xa5; 60]).unwrap();
    cpu.regs[1] = handle;
    cpu.regs[2] = out;
    cpu.regs[8] = BASE + 4096; // invalid second output, valid first and third
    cpu.regs[9] = out + 16;
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "GetFileTime", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
    for (class, size) in [(0, 40), (1, 24), (9, 8), (18, 24)] {
        mem.write(out, &[0xa5; 60]).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetFileInformationByHandleEx",
                &[handle, class, out, size - 1]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            24
        );
        assert_eq!(mem.read_int(out, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetFileInformationByHandleEx",
                &[handle, class, out, size]
            ),
            1
        );
        assert_eq!(mem.read_int(out + size, 4).unwrap(), 0xa5a5a5a5);
        match class {
            0 => {
                assert_eq!(mem.read_int(out, 8).unwrap(), times[0]);
                assert_eq!(mem.read_int(out + 16, 8).unwrap(), times[2]);
                assert_ne!(mem.read_int(out + 24, 8).unwrap(), 0);
                assert_eq!(mem.read_int(out + 32, 4).unwrap(), 0x80);
            }
            1 => {
                assert_eq!(mem.read_int(out, 8).unwrap(), meta.blocks() * 512);
                assert_eq!(mem.read_int(out + 8, 8).unwrap(), length);
                assert_eq!(mem.read_int(out + 16, 4).unwrap(), 2);
                assert_eq!(mem.read_int(out + 20, 4).unwrap(), 0);
            }
            9 => assert_eq!(mem.read_int(out, 8).unwrap(), 0x80),
            18 => {
                assert_eq!(mem.read_int(out, 8).unwrap(), meta.dev());
                assert_eq!(mem.read_int(out + 8, 8).unwrap(), meta.ino());
                assert_eq!(mem.read_int(out + 16, 8).unwrap(), 0);
            }
            _ => unreachable!(),
        }
    }
    mem.write(name, b"C:\\alias.bin\0").unwrap();
    let alias = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[name, 0x80000000, 7, 0, 3, 0x80, 0],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileInformationByHandle",
            &[alias, out]
        ),
        1
    );
    assert_eq!(
        mem.read_int(out + 44, 4).unwrap() << 32 | mem.read_int(out + 48, 4).unwrap(),
        index
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileInformationByHandleEx",
            &[handle, 2, out, 60]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        50
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[handle]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileInformationByHandle",
            &[handle, out]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        6
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]);
    drop(file);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn win64_file_io_is_real_checks_sharing_and_preflights_before_advancing_offsets() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let root = std::env::temp_dir().join(format!("wire64-io-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let path = STACK + 0x100;
    let bytes = STACK + 0x200;
    let count = STACK + 0x300;
    mem.write(path, b"C:\\data.bin\0").unwrap();
    mem.write(bytes, b"actual-file").unwrap();
    let writer = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[path, 0x40000000, 0, 0, 2, 0x80, 0],
    );
    assert_ne!(writer, u64::MAX);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WriteFile",
            &[writer, bytes, 11, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 11);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateFileA",
            &[path, 0x80000000, 3, 0, 3, 0x80, 0]
        ),
        u64::MAX
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        32
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[writer]),
        1
    );
    assert_eq!(
        std::fs::read(root.join("data.bin")).unwrap(),
        b"actual-file"
    );
    let reader = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateFileA",
        &[path, 0x120089, 3, 0, 3, 0x80, 0],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileSizeEx",
            &[reader, count]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 8).unwrap(), 11);
    cpu.regs[1] = reader;
    cpu.regs[2] = bytes;
    cpu.regs[8] = 11;
    cpu.regs[9] = 0;
    mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
    mem.write_int(cpu.regs[4] + 40, 8, 0).unwrap();
    assert!(runtime
        .dispatch("kernel32.dll", "ReadFile", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[reader, bytes, 11, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 11);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[reader, bytes, 11, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetFilePointerEx",
            &[reader, (-4i64) as u64, count, 2]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 8).unwrap(), 7);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, reader, u64::MAX, count, 0, 1, 2]
        ),
        1
    );
    let duplicate = mem.read_int(count, 8).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[duplicate, count]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 1);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[duplicate, bytes, 4, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 4);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReadFile",
            &[reader, bytes, 4, count, 0]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 4).unwrap(), 0); // Shared underlying file offset.
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[reader]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileSizeEx",
            &[duplicate, count]
        ),
        1
    );
    assert_eq!(mem.read_int(count, 8).unwrap(), 11);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CloseHandle",
        &[duplicate],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, 0x101, u64::MAX, count, 0, 0, 2]
        ),
        1
    );
    let duplicate = mem.read_int(count, 8).unwrap();
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[0x101]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileType",
            &[duplicate]
        ),
        3
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WriteFile",
            &[duplicate, bytes, 0, count, 0]
        ),
        1
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CloseHandle",
        &[duplicate],
    );
    std::fs::remove_dir_all(root).unwrap();
}
