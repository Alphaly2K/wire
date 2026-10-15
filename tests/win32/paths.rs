//! Path-helper behavior baselines, independent of the API catalog.
use super::*;

#[test]
fn path_helpers_keep_stdcall_null_shortcuts_and_provider_scope() {
    let mut v = Vm::new();
    for (dll, name, slots, error) in [
        ("shlwapi.dll", "PathFileExistsA", 1, 87),
        ("shlwapi.dll", "PathFileExistsW", 1, 87),
        ("shlwapi.dll", "PathIsDirectoryA", 1, 87),
        ("shlwapi.dll", "PathIsDirectoryW", 1, 87),
        ("shlwapi.dll", "PathRemoveFileSpecA", 1, 0x55),
        ("shlwapi.dll", "PathRemoveFileSpecW", 1, 0x55),
        ("shlwapi.dll", "PathRenameExtensionA", 2, 0x55),
        ("shlwapi.dll", "PathRenameExtensionW", 2, 0x55),
        ("shlwapi.dll", "PathAddExtensionA", 2, 0x55),
        ("shlwapi.dll", "PathAddExtensionW", 2, 0x55),
        ("shlwapi.dll", "PathUnquoteSpacesA", 1, 0x55),
        ("shlwapi.dll", "PathUnquoteSpacesW", 1, 0x55),
        ("imagehlp.dll", "MakeSureDirectoryPathExists", 1, 87),
    ] {
        assert!(api::native_export(dll, name));
        assert!(!api::native_export("user32.dll", name));
        v.k("SetLastError", &[0x55]);
        // NULL path short-circuits the otherwise invalid extension pointer.
        assert_eq!(v.call(dll, name, &[0, 0x700000]), 0, "{name}");
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4 + slots * 4);
        assert_eq!(v.cpu.eip, RET);
        assert_eq!(v.state.last_error, error, "{name}");
        assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), error);
        assert!(
            wire::file_api::dispatch("user32.dll", name, &[0; 16], &v.mem, &mut v.state).is_none()
        );
    }
    for name in [
        "PathRenameExtensionA",
        "PathRenameExtensionW",
        "PathAddExtensionA",
        "PathAddExtensionW",
    ] {
        v.k("SetLastError", &[0x55]);
        assert_eq!(v.call("shlwapi.dll", name, &[0x700000, 0]), 0);
        assert_eq!(v.state.last_error, 0x55);
    }
    // The public facade historically accepts lowercase providers only.
    assert!(wire::file_api::dispatch(
        "SHLWAPI.DLL",
        "PathRemoveFileSpecA",
        &[0; 16],
        &v.mem,
        &mut v.state
    )
    .is_none());
}

#[test]
fn path_helpers_keep_output_faults_length_units_and_void_return() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, b"C:\\dir\\name.old\0").unwrap();
    v.mem.write_bytes(DATA + 0x1000, b".new\0").unwrap();
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_READONLY)
        .unwrap();
    for name in ["PathRemoveFileSpecA", "PathRenameExtensionA"] {
        v.k("SetLastError", &[0x55]);
        assert_eq!(v.call("shlwapi.dll", name, &[DATA, DATA + 0x1000]), 0);
        assert_eq!(v.state.last_error, 87);
        assert_eq!(v.mem.read_cstr(DATA, 260).unwrap(), "C:\\dir\\name.old");
    }
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_READWRITE)
        .unwrap();
    for wide in [false, true] {
        // CP932 bytes versus UTF-16 units: both exceed MAX_PATH after append.
        let text = if wide {
            "a".repeat(258)
        } else {
            "ソ".repeat(129)
        };
        if wide {
            v.wide(DATA, &text);
            v.wide(DATA + 0x1000, ".x");
        } else {
            let mut bytes = wire::codepage::encode(&text, 932).unwrap().0;
            bytes.push(0);
            v.mem.write_bytes(DATA, &bytes).unwrap();
            v.mem.write_bytes(DATA + 0x1000, b".x\0").unwrap();
        }
        v.mem
            .virtual_protect(DATA, 4096, wire::mem::PAGE_READONLY)
            .unwrap();
        assert_eq!(
            v.call(
                "shlwapi.dll",
                if wide {
                    "PathAddExtensionW"
                } else {
                    "PathAddExtensionA"
                },
                &[DATA, DATA + 0x1000]
            ),
            0
        );
        assert_eq!(v.state.last_error, 206); // length check precedes output fault
        assert_eq!(wire::file_api::read_name(&v.mem, DATA, wide).unwrap(), text);
        v.mem
            .virtual_protect(DATA, 4096, wire::mem::PAGE_READWRITE)
            .unwrap();
    }
    v.wide(DATA, "\"ソ path\"");
    v.k("SetLastError", &[0x55]);
    assert_eq!(v.call("shlwapi.dll", "PathUnquoteSpacesW", &[DATA]), 0);
    assert_eq!(v.state.last_error, 0x55);
    assert_eq!(v.mem.read_wstr(DATA, 260).unwrap(), "ソ path");
}

#[test]
fn path_helpers_keep_filesystem_queries_errors_and_directory_suffix() {
    let mut v = Vm::new();
    std::fs::write(v.dir.join("file.dat"), b"data").unwrap();
    std::fs::create_dir(v.dir.join("directory")).unwrap();
    for (path, exists, directory, error) in [
        ("C:\\file.dat", 1, 0, 0x55),
        ("C:\\directory", 1, 1, 0x55),
        ("C:\\missing", 0, 0, 2),
    ] {
        for wide in [false, true] {
            if wide {
                v.wide(DATA, path);
            } else {
                v.mem
                    .write_bytes(DATA, format!("{path}\0").as_bytes())
                    .unwrap();
            }
            for (name, expected) in if wide {
                [("PathFileExistsW", exists), ("PathIsDirectoryW", directory)]
            } else {
                [("PathFileExistsA", exists), ("PathIsDirectoryA", directory)]
            } {
                v.k("SetLastError", &[0x55]);
                assert_eq!(v.call("shlwapi.dll", name, &[DATA]), expected);
                assert_eq!(v.state.last_error, error);
            }
        }
    }
    v.mem.write_bytes(DATA, b"C:\\created\\child\\\0").unwrap();
    assert_eq!(
        v.call("imagehlp.dll", "MakeSureDirectoryPathExists", &[DATA]),
        1
    );
    assert!(v.dir.join("created/child").is_dir());
}
