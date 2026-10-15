//! Existing behavioral tests grouped by files; expectations remain independent of API catalogs.

use super::*;

#[test]
fn search_contract_baseline_keeps_pe32_cleanup_flags_and_handle_type() {
    let mut v = Vm::new();
    std::fs::write(v.dir.join("a.bound"), b"a").unwrap();
    std::fs::write(v.dir.join("b.bound"), b"b").unwrap();
    let output = DATA + 512;
    v.mem.write_bytes(output, &[0xa5; 320]).unwrap();
    // Invalid Ex flags precede path decoding and any output access.
    assert_eq!(
        v.k("FindFirstFileExA", &[0x30000, 0, output, 0, 0, 4]),
        u32::MAX
    );
    assert_eq!(v.state.last_error, 87);
    assert_eq!(v.mem.read_u32(output).unwrap(), 0xa5a5a5a5);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 28);
    v.mem.write_bytes(DATA, b"C:\\*.bound\0").unwrap();
    v.k("SetLastError", &[0x55]);
    let search = v.k("FindFirstFileExA", &[DATA, 1, output, 0, 0, 2]);
    assert_ne!(search, u32::MAX);
    assert_eq!(v.state.last_error, 0x55);
    assert_eq!(v.mem.read_cstr(output + 44, 260).unwrap(), "a.bound");
    assert_eq!(v.k("FindNextFileA", &[search, output]), 1);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 12);
    assert_eq!(v.mem.read_cstr(output + 44, 260).unwrap(), "b.bound");
    assert_eq!(v.k("FindClose", &[search]), 1);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 8);
    assert_eq!(v.k("FindNextFileA", &[search, 0x30000]), 0);
    assert_eq!(v.state.last_error, 6);

    v.mem.write_bytes(DATA, b"C:\\a.bound\0").unwrap();
    let file = v.k("CreateFileA", &[DATA, 0x80000000, 0, 0, 3, 0x80, 0]);
    assert_ne!(file, u32::MAX);
    assert_eq!(v.k("FindClose", &[file]), 0);
    assert_eq!(v.state.last_error, 6);
    assert_eq!(v.k("ReadFile", &[file, DATA + 128, 1, DATA + 256, 0]), 1);
    assert_eq!(v.mem.read_u8(DATA + 128).unwrap(), b'a');
    assert_eq!(v.k("CloseHandle", &[file]), 1);
    for name in [
        "FindFirstFileA",
        "FindFirstFileW",
        "FindFirstFileExA",
        "FindFirstFileExW",
        "FindNextFileA",
        "FindNextFileW",
        "FindClose",
    ] {
        assert_eq!(v.call("kernelbase.dll", name, &[0x30000; 6]), 0);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
    }
}

#[test]
fn abi_only_file_binding_rebinds_when_public_entry_changes_execution_policy() {
    let mut v = Vm::new();
    assert_eq!(v.k("GetFileAttributesExA", &[0x30000, 1, DATA]), 0);
    let address = v
        .api
        .lookup("kernel32.dll", "GetFileAttributesExA")
        .unwrap();
    let index = v.api.stub_index(address).unwrap();
    v.api.entries[index].name = "GetCurrentDirectoryA".into();
    // register() still returns the original public stub; dispatch must rebind it.
    assert_eq!(v.k("GetFileAttributesExA", &[4, DATA, 0]), 3);
    assert_eq!(v.mem.read_cstr(DATA, 4).unwrap(), "C:\\");
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 12);
    v.api.entries[index].name = "GetFileAttributesExA".into();
    v.mem.write_u32(DATA, 0xa5a5a5a5).unwrap();
    assert_eq!(v.k("GetFileAttributesExA", &[0x30000, 1, DATA]), 0);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 16);
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
}

#[test]
fn mutation_contract_baseline_keeps_pe32_security_subset_and_abi_only_calls() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, b"C:\\fresh\0").unwrap();
    v.k("SetLastError", &[0x55]);
    // PE32's existing directory subset ignores SECURITY_ATTRIBUTES entirely.
    assert_eq!(v.k("CreateDirectoryA", &[DATA, 0x30000]), 1);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 12);
    assert!(v.dir.join("fresh").is_dir());
    assert_eq!(v.state.last_error, 0x55);
    for (name, slots) in [
        ("MoveFileA", 2),
        ("MoveFileW", 2),
        ("MoveFileExA", 3),
        ("MoveFileExW", 3),
        ("CopyFileA", 3),
        ("CopyFileW", 3),
        ("RemoveDirectoryA", 1),
        ("RemoveDirectoryW", 1),
    ] {
        assert_eq!(v.k(name, &[0x30000; 3][..slots]), 0);
        assert_eq!(
            v.cpu.esp(),
            STACK_BASE + STACK_SIZE - 256 + 4 + slots as u32 * 4
        );
        assert_eq!(v.state.last_error, 120);
        assert!(v.dir.join("fresh").is_dir());
    }
    assert_eq!(v.call("user32.dll", "CreateDirectoryA", &[DATA, 0]), 0);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
    assert!(v.dir.join("fresh").is_dir());
}

#[test]
fn metadata_contract_baseline_keeps_pe32_abi_only_outputs_and_unknown_dll_cleanup() {
    let mut v = Vm::new();
    for (name, slots) in [
        ("GetFileAttributesExA", 3),
        ("GetFileAttributesExW", 3),
        ("GetFileInformationByHandle", 2),
        ("GetFileInformationByHandleEx", 4),
        ("GetFileTime", 4),
    ] {
        v.mem.write_bytes(DATA, &[0xa5; 64]).unwrap();
        v.k("SetLastError", &[0x55]);
        // ABI-only calls ignore even invalid guest pointers and preserve outputs.
        assert_eq!(v.k(name, &[0x30000, 99, DATA, 64][..slots]), 0);
        assert_eq!(v.cpu.eip, RET);
        assert_eq!(
            v.cpu.esp(),
            STACK_BASE + STACK_SIZE - 256 + 4 + slots as u32 * 4
        );
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.mem.read_u64(DATA).unwrap(), 0xa5a5a5a5a5a5a5a5);
        assert_eq!(
            v.log.unimpl_freq.get(&format!("kernel32.dll!{name}")),
            Some(&1)
        );
        assert_eq!(v.call("kernelbase.dll", name, &[0x30000; 4]), 0);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
    }
}

#[test]
fn path_contract_baseline_keeps_full_path_partial_output_and_stdcall() {
    let mut v = Vm::new();
    let output = DATA + 0x400;
    let part = DATA + 0x500;
    v.mem.write_bytes(DATA, b"leaf.dat\0").unwrap();
    v.mem.write_bytes(output, &[0xaa; 64]).unwrap();
    v.mem.write_u32(part, 0xfeedface).unwrap();
    v.k("SetLastError", &[0x55]);
    assert_eq!(v.k("GetFullPathNameA", &[DATA, 2, output, part]), 12);
    assert_eq!(v.mem.read_u32(output).unwrap(), 0xaaaaaaaa);
    assert_eq!(v.mem.read_u32(part).unwrap(), 0xfeedface);
    assert_eq!(v.state.last_error, 0x55);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 20);
    assert_eq!(v.k("GetFullPathNameA", &[DATA, 64, output, 0x30000]), 0);
    assert_eq!(v.state.last_error, 87);
    assert_eq!(v.mem.read_cstr(output, 64).unwrap(), "C:\\leaf.dat");
    v.wide(DATA, "leaf.dat");
    assert_eq!(v.k("GetFullPathNameW", &[DATA, 64, output, part]), 11);
    assert_eq!(v.mem.read_wstr(output, 64).unwrap(), "C:\\leaf.dat");
    assert_eq!(v.mem.read_u32(part).unwrap(), output + 6);
    v.mem.write_bytes(output, &[0xaa; 64]).unwrap();
    assert_eq!(
        v.call("user32.dll", "GetFullPathNameW", &[DATA, 64, output, part]),
        0
    );
    assert_eq!(v.mem.read_u32(output).unwrap(), 0xaaaaaaaa);
}

#[test]
fn core_file_contract_keeps_pe32_split_distance_and_post_seek_output_failure() {
    let mut v = Vm::new();
    std::fs::write(v.dir.join("seek.bin"), b"abc").unwrap();
    v.wide(DATA, "C:\\seek.bin");
    let file = v.k("CreateFileW", &[DATA, 0x80000000, 0, 0, 3, 0x80, 0]);
    assert_ne!(file, u32::MAX);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 32);
    assert_eq!(v.k("SetFilePointerEx", &[file, 2, 1, 0x50000, 0]), 0);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 24);
    assert_eq!(v.k("GetLastError", &[]), 87);
    // PE32's existing seek commits before checking its optional output.
    assert_eq!(v.k("SetFilePointerEx", &[file, 0, 0, DATA + 128, 1]), 1);
    assert_eq!(v.mem.read_u64(DATA + 128).unwrap(), 0x1_0000_0002);
    v.k("SetLastError", &[0x1234]);
    assert_eq!(v.k("GetFileType", &[0xdeadbeef]), 0);
    assert_eq!(v.k("GetLastError", &[]), 0x1234);
    assert_eq!(v.call("user32.dll", "GetFileType", &[file]), 0);
    assert_eq!(v.log.unimpl_freq.get("user32.dll!GetFileType"), Some(&1));
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
    assert_eq!(v.k("GetFileType", &[file]), 1);
    assert_eq!(v.k("CloseHandle", &[file]), 1);
}

#[test]
fn file_buffers_search_cursor_and_semaphore_outputs_keep_fault_commit_order() {
    let mut v = Vm::new();
    std::fs::write(v.dir.join("a.order"), b"abc").unwrap();
    std::fs::write(v.dir.join("b.order"), b"def").unwrap();
    v.wide(DATA, "C:\\a.order");
    let file = v.k("CreateFileW", &[DATA, 0x80000000, 0, 0, 3, 0x80, 0]);
    v.mem.write_u32(DATA + 128, 0xdeadbeef).unwrap();
    assert_eq!(v.k("ReadFile", &[file, 0, 3, DATA + 128, 0]), 0);
    assert_eq!(v.mem.read_u32(DATA + 128).unwrap(), 0);
    assert_eq!(v.k("ReadFile", &[file, DATA + 256, 3, DATA + 128, 0]), 1);
    let mut bytes = [0; 3];
    v.mem.read_bytes(DATA + 256, &mut bytes).unwrap();
    assert_eq!(&bytes, b"abc"); // The failed buffer access did not advance the file.

    v.wide(DATA, "C:\\*.order");
    let search = v.k("FindFirstFileW", &[DATA, DATA + 512]);
    assert_ne!(search, u32::MAX);
    assert_eq!(v.k("FindNextFileW", &[search, 0]), 0);
    assert_eq!(v.k("FindNextFileW", &[search, DATA + 512]), 1);
    assert_eq!(v.mem.read_wstr(DATA + 512 + 44, 260).unwrap(), "b.order");
    assert_eq!(v.k("FindNextFileW", &[search, DATA + 512]), 0);
    assert_eq!(v.k("GetLastError", &[]), 18);
    assert_eq!(v.k("CloseHandle", &[search]), 1);
    assert_eq!(v.k("FindClose", &[search]), 0);

    let sem = v.k("CreateSemaphoreW", &[0, 0, 2, 0]);
    let mut registers = v.cpu.regs;
    registers[wire::cpu::ESP] = STACK_BASE + STACK_SIZE - 256;
    assert!(v
        .try_call("kernel32.dll", "ReleaseSemaphore", &[sem, 1, 0x50000])
        .is_err());
    assert_eq!(
        v.cpu.eip,
        v.api.lookup("kernel32.dll", "ReleaseSemaphore").unwrap()
    );
    assert_eq!(v.cpu.regs, registers);
    assert_eq!(v.k("WaitForSingleObject", &[sem, 0]), 0);
    assert_eq!(v.k("WaitForSingleObject", &[sem, 0]), 0x102);
}

#[test]
fn files_read_write_seek_eof_and_invalid_handles() {
    let mut v = Vm::new();
    v.wide(DATA, "C:\\日本.txt");
    let h = v.k("CreateFileW", &[DATA, 0xc0000000, 0, 0, 2, 0x80, 0]);
    assert_ne!(h, u32::MAX);
    v.mem.write_bytes(DATA + 256, b"abc123").unwrap();
    assert_eq!(v.k("WriteFile", &[h, DATA + 256, 6, DATA + 512, 0]), 1);
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 6);
    assert_eq!(v.k("GetFileType", &[h]), 1);
    assert_eq!(v.k("GetFileSizeEx", &[h, DATA + 520]), 1);
    assert_eq!(v.mem.read_u64(DATA + 520).unwrap(), 6);
    assert_eq!(v.k("SetFilePointer", &[h, 3, 0, 0]), 3);
    assert_eq!(v.k("ReadFile", &[h, DATA + 300, 10, DATA + 512, 0]), 1);
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 3);
    let mut b = [0; 3];
    v.mem.read_bytes(DATA + 300, &mut b).unwrap();
    assert_eq!(&b, b"123");
    assert_eq!(v.k("ReadFile", &[h, DATA + 300, 10, DATA + 512, 0]), 1);
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 0);
    assert_eq!(v.k("SetFilePointerEx", &[h, 7, 1, DATA + 520, 0]), 1);
    assert_eq!(v.mem.read_u64(DATA + 520).unwrap(), 0x100000007);
    assert_eq!(v.k("SetFilePointerEx", &[h, u32::MAX, u32::MAX, 0, 0]), 0);
    assert_eq!(v.k("CloseHandle", &[h]), 1);
    assert_eq!(v.k("ReadFile", &[h, DATA + 300, 1, DATA + 512, 0]), 0);
    assert_eq!(v.k("GetLastError", &[]), 6);
    assert_eq!(std::fs::read(v.dir.join("日本.txt")).unwrap(), b"abc123");
}

#[test]
fn filesystem_confines_paths_and_symlinks_and_handles_case() {
    let mut v = Vm::new();
    std::fs::create_dir(v.dir.join("Sub")).unwrap();
    std::fs::write(v.dir.join("Sub/Test.TXT"), b"ok").unwrap();
    let fs = v.state.fs.as_mut().unwrap();
    assert_eq!(
        fs.resolve("c:\\sub\\test.txt").unwrap(),
        v.dir.join("Sub/Test.TXT")
    );
    assert_eq!(
        fs.resolve("\\\\?\\c:\\sub\\test.txt").unwrap(),
        v.dir.join("Sub/Test.TXT")
    );
    assert_eq!(
        fs.resolve("\\\\?\\C:\\Sub\\new-file").unwrap(),
        v.dir.join("Sub/new-file")
    );
    for unsupported in [
        "\\\\?\\UNC\\server\\file",
        "\\\\?\\D:\\file",
        "\\\\?\\C:relative",
        "\\\\.\\C:\\file",
        "\\\\?\\GLOBALROOT\\Device\\file",
        "\\\\?\\C:\\..\\outside",
    ] {
        assert!(fs.resolve(unsupported).is_err(), "{unsupported}");
    }
    assert!(fs.resolve("../outside").is_err());
    assert!(fs.resolve("C:\\..\\outside").is_err());
    assert!(fs.resolve("\\\\server\\file").is_err());
    fs.set_cwd("Sub").unwrap();
    assert_eq!(fs.cwd(), "C:\\Sub");
    assert_eq!(
        fs.resolve("../Sub/test.txt").unwrap(),
        v.dir.join("Sub/Test.TXT")
    );
    std::os::unix::fs::symlink(std::env::temp_dir(), v.dir.join("escape")).unwrap();
    assert!(fs.resolve("C:\\escape\\new-file").is_err());
    assert!(fs.resolve("C:\\escape").is_err());
    assert!(fs.resolve("\\\\?\\C:\\escape\\new-file").is_err());
}

#[test]
fn directory_queries_and_enumeration() {
    let mut v = Vm::new();
    std::fs::write(v.dir.join("one.cf"), b"cfg").unwrap();
    std::fs::write(v.dir.join("two.cf"), b"cfg2").unwrap();
    v.wide(DATA, "C:\\*.cf");
    let h = v.k("FindFirstFileW", &[DATA, DATA + 256]);
    assert_ne!(h, u32::MAX);
    assert_eq!(v.mem.read_wstr(DATA + 256 + 44, 260).unwrap(), "one.cf");
    assert_eq!(v.k("FindNextFileW", &[h, DATA + 256]), 1);
    assert_eq!(v.mem.read_wstr(DATA + 256 + 44, 260).unwrap(), "two.cf");
    assert_eq!(v.k("FindNextFileW", &[h, DATA + 256]), 0);
    assert_eq!(v.k("GetLastError", &[]), 18);
    assert_eq!(v.k("FindClose", &[h]), 1);
    assert_eq!(v.k("GetCurrentDirectoryW", &[2, DATA + 256]), 4);
    assert_eq!(v.k("GetCurrentDirectoryW", &[4, DATA + 256]), 3);
    assert_eq!(v.mem.read_wstr(DATA + 256, 4).unwrap(), "C:\\");
    let h = v.k("FindFirstFileExW", &[DATA, 1, DATA + 256, 0, 0, 2]);
    assert_ne!(h, u32::MAX);
    assert_eq!(v.mem.read_wstr(DATA + 256 + 44, 260).unwrap(), "one.cf");
    assert_eq!(v.k("FindClose", &[h]), 1);
    assert_eq!(
        v.k("FindFirstFileExW", &[DATA, 2, DATA + 256, 0, 0, 0]),
        u32::MAX
    );
    assert_eq!(v.k("GetLastError", &[]), 87);
}

#[test]
fn path_directory_creation_readonly_and_delete_stay_inside_guest_root() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(DATA, b"C:\\nested\\child\\save.dat\0")
        .unwrap();
    assert_eq!(
        v.call("imagehlp.dll", "MakeSureDirectoryPathExists", &[DATA]),
        1
    );
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 8);
    assert!(v.dir.join("nested/child").is_dir());
    assert!(!v.dir.join("nested/child/save.dat").exists());
    std::fs::write(v.dir.join("nested/child/save.dat"), b"save").unwrap();
    assert_eq!(v.k("SetFileAttributesA", &[DATA, 1]), 1);
    assert_eq!(v.k("DeleteFileA", &[DATA]), 0);
    assert_eq!(v.state.last_error, 5);
    assert_eq!(v.k("SetFileAttributesA", &[DATA, 0x80]), 1);
    assert_eq!(v.k("DeleteFileA", &[DATA]), 1);
    assert_eq!(v.k("DeleteFileA", &[DATA]), 0);
    v.mem.write_bytes(DATA + 0x400, b".bak\0").unwrap();
    assert_eq!(
        v.call("shlwapi.dll", "PathRenameExtensionA", &[DATA, DATA + 0x400]),
        1
    );
    assert!(v.mem.read_cstr(DATA, 260).unwrap().ends_with("save.bak"));
    assert_eq!(
        v.call("winmm.dll", "mciSendCommandW", &[0, 0x803, 0, 0]),
        274
    );
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 20);
}
