//! Existing behavioral tests grouped by kernel; expectations remain independent of API catalogs.

use super::*;

#[test]
fn system_profile_keeps_pe32_layouts_public_dispatch_and_output_errors() {
    let mut v = Vm::new();
    v.k("SetLastError", &[77]);
    for name in ["GetSystemInfo", "GetNativeSystemInfo"] {
        v.mem.write_bytes(DATA, &[0xa5; 56]).unwrap();
        assert_eq!(v.k(name, &[DATA]), 0);
        for (offset, value) in [
            (0, 0),
            (4, 4096),
            (8, 0x10000),
            (12, 0x7ffeffff),
            (16, 1),
            (20, 1),
            (24, 586),
            (28, 65536),
        ] {
            assert_eq!(
                v.mem.read_u32(DATA + offset).unwrap(),
                value,
                "{name}+{offset}"
            );
        }
        assert_eq!(v.mem.read_u16(DATA + 32).unwrap(), 6);
        assert_eq!(v.mem.read_u16(DATA + 34).unwrap(), 0x3a09);
        assert_eq!(v.mem.read_u32(DATA + 36).unwrap(), 0xa5a5a5a5);
        assert_eq!(v.state.last_error, 77);
    }
    let mut args = [0; 16];
    args[0] = DATA;
    assert!(wire::system::dispatch(
        "kernel32.dll",
        "GetLogicalProcessorInformationEx",
        &args,
        &v.mem,
        &mut v.state
    )
    .is_none());
    assert!(
        wire::system::dispatch("KERNEL32.dll", "GetSystemInfo", &args, &v.mem, &mut v.state)
            .is_none()
    );
    assert!(wire::system::dispatch(
        "kernelbase.dll",
        "GetSystemInfo",
        &args,
        &v.mem,
        &mut v.state
    )
    .is_none());
    let result =
        wire::system::dispatch("kernel32.dll", "GetSystemInfo", &args, &v.mem, &mut v.state)
            .unwrap();
    assert_eq!(
        result,
        (0, "x86 guest hardware / monotonic clock profile".into())
    );
    let sp = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.k("GetSystemInfo", &[sp]), 0);
    assert_eq!(v.cpu.eip, 0); // Legacy return slot now contains processor architecture.
    assert_eq!(v.cpu.esp(), sp + 8);
    assert_eq!(v.k("GetSystemInfo", &[0]), 0);
    assert_eq!(v.state.last_error, 87);
    assert_eq!(v.k("GlobalMemoryStatusEx", &[0]), 0);
    assert_eq!(v.state.last_error, 87);
    v.mem.write_bytes(DATA, &[0xa5; 72]).unwrap();
    assert_eq!(v.k("GlobalMemoryStatus", &[DATA]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 32);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 512 * 1024 * 1024);
    assert_eq!(v.mem.read_u32(DATA + 24).unwrap(), 0x7fff0000);
    assert_eq!(v.mem.read_u32(DATA + 32).unwrap(), 0xa5a5a5a5);
    v.mem.write_u32(DATA, 63).unwrap();
    let mut before = [0; 64];
    v.mem.read_bytes(DATA, &mut before).unwrap();
    assert_eq!(v.k("GlobalMemoryStatusEx", &[DATA]), 0);
    let mut after = [0; 64];
    v.mem.read_bytes(DATA, &mut after).unwrap();
    assert_eq!(after, before);
    v.mem.write_u32(DATA, 64).unwrap();
    assert_eq!(v.k("GlobalMemoryStatusEx", &[DATA]), 1);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 512 * 1024 * 1024);
    assert_eq!(v.mem.read_u32(DATA + 12).unwrap(), 0);
    assert_eq!(v.mem.read_u32(DATA + 40).unwrap(), 0x7fff0000);
    assert_eq!(v.mem.read_u32(DATA + 44).unwrap(), 0);
    assert_eq!(v.mem.read_u32(DATA + 64).unwrap(), 0xa5a5a5a5);
    assert_eq!(v.state.last_error, 87);
}

#[test]
fn wine_path_extensions_and_unquoting_preserve_existing_names() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, b"dir.with.dot\\name\0").unwrap();
    v.mem.write_bytes(DATA + 0x100, b".pgd\0").unwrap();
    assert_eq!(
        v.call("shlwapi.dll", "PathAddExtensionA", &[DATA, DATA + 0x100]),
        1
    );
    assert_eq!(
        v.mem.read_cstr(DATA, 260).unwrap(),
        "dir.with.dot\\name.pgd"
    );
    assert_eq!(
        v.call("shlwapi.dll", "PathAddExtensionA", &[DATA, DATA + 0x100]),
        0
    );
    v.wide(DATA, "\"ソフト path\"");
    v.call("shlwapi.dll", "PathUnquoteSpacesW", &[DATA]);
    assert_eq!(v.mem.read_wstr(DATA, 260).unwrap(), "ソフト path");
    v.mem.write_bytes(DATA, b"\"incomplete\0").unwrap();
    v.call("shlwapi.dll", "PathUnquoteSpacesA", &[DATA]);
    assert_eq!(v.mem.read_cstr(DATA, 260).unwrap(), "\"incomplete");
}

#[test]
fn missing_registry_installations_fail_and_dsound_ordinals_resolve() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(DATA, b"Software\\not-installed\0")
        .unwrap();
    v.mem.write_u32(DATA + 0x100, 0xdeadbeef).unwrap();
    assert_eq!(
        v.call(
            "advapi32.dll",
            "RegOpenKeyA",
            &[0x80000002, DATA, DATA + 0x100]
        ),
        2
    );
    assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 0);
    assert_eq!(
        v.call(
            "advapi32.dll",
            "RegQueryValueExA",
            &[0, DATA, 0, 0, 0, DATA + 0x100]
        ),
        6
    );
    v.mem.write_bytes(DATA, b"dsound.dll\0").unwrap();
    let module = v.k("LoadLibraryA", &[DATA]);
    let ordinal = v.k("GetProcAddress", &[module, 1]);
    assert_ne!(ordinal, 0);
    assert_eq!(ordinal, dll_export(&mut v, module, "DirectSoundCreate"));
    assert_eq!(
        v.call("dsound.dll", "DirectSoundCreate", &[0, DATA + 0x100, 0]),
        0
    );
    let sound = v.mem.read_u32(DATA + 0x100).unwrap();
    assert_ne!(sound, 0);
    assert_eq!(d3d_com(&mut v, sound, 2, &[]), 0);
}

#[test]
fn path_remove_file_spec_preserves_roots_dbcs_and_checked_buffers() {
    let mut v = Vm::new();
    for (input, expected, changed) in [
        ("C:\\koikake.exe", "C:\\", 1),
        ("C:\\dll\\Pal.dll", "C:\\dll", 1),
        ("C:\\", "C:\\", 0),
        ("C:", "C:", 0),
        ("file.exe", "", 1),
        ("\\\\server\\share\\file.exe", "\\\\server\\share", 1),
        ("C:\\dir\\\\file.exe", "C:\\dir", 1),
        ("C:\\ソフト\\ソ", "C:\\ソフト", 1),
        ("C:\\dir/file.exe", "C:\\", 1),
        ("", "", 0),
    ] {
        for wide in [false, true] {
            if wide {
                v.wide(DATA, input);
            } else {
                let mut bytes = wire::codepage::encode(input, 932).unwrap().0;
                bytes.push(0);
                v.mem.write_bytes(DATA, &bytes).unwrap();
            }
            assert_eq!(
                v.call(
                    "shlwapi.dll",
                    if wide {
                        "PathRemoveFileSpecW"
                    } else {
                        "PathRemoveFileSpecA"
                    },
                    &[DATA]
                ),
                changed,
                "{input:?} wide {wide}"
            );
            assert_eq!(
                wire::file_api::read_name(&v.mem, DATA, wide).unwrap(),
                expected
            );
        }
    }
    v.wide(DATA, "\\\\?\\C:\\file.exe");
    assert_eq!(v.call("shlwapi.dll", "PathRemoveFileSpecW", &[DATA]), 1);
    assert_eq!(v.mem.read_wstr(DATA, 128).unwrap(), "\\\\?\\C:\\");
    assert_eq!(v.call("shlwapi.dll", "PathRemoveFileSpecA", &[0]), 0);
    assert_eq!(v.call("shlwapi.dll", "PathRemoveFileSpecW", &[0x700000]), 0);
}

#[test]
fn common_strings_arithmetic_com_apartments_and_guid_use_checked_state() {
    let mut v = Vm::new();
    v.wide(DATA, "AbC\0tail");
    v.wide(DATA + 64, "abc\0tail");
    assert_eq!(v.k("CompareStringOrdinal", &[DATA, 3, DATA + 64, 3, 1]), 2);
    assert_eq!(v.k("CompareStringOrdinal", &[DATA, 3, DATA + 64, 3, 0]), 1);
    assert_eq!(v.k("CompareStringOrdinal", &[DATA, 0, DATA + 64, 0, 0]), 2);
    assert_eq!(v.k("CompareStringOrdinal", &[0, 0, DATA, 0, 0]), 0);
    for (n, multiplier, divisor, expected) in [
        (5, 1, 2, 3),
        (-5, 1, 2, -3),
        (5, 1, -2, -3),
        (i32::MIN, 1, 1, i32::MIN),
        (i32::MAX, 2, 1, -1),
        (1, 1, 0, -1),
    ] {
        assert_eq!(
            v.k("MulDiv", &[n as u32, multiplier as u32, divisor as u32]),
            expected as u32
        );
    }
    assert_eq!(v.k("IsBadReadPtr", &[0, 0]), 0);
    assert_eq!(v.k("IsBadReadPtr", &[0, 1]), 1);
    assert_eq!(v.k("IsBadWritePtr", &[DATA, 4]), 0);
    assert_eq!(v.call("ole32.dll", "CoInitializeEx", &[0, 2]), 0);
    assert_eq!(v.call("ole32.dll", "CoInitializeEx", &[0, 2]), 1);
    assert_eq!(v.call("ole32.dll", "CoInitializeEx", &[0, 0]), 0x80010106);
    v.call("ole32.dll", "CoUninitialize", &[]);
    assert_eq!(v.call("ole32.dll", "CoInitializeEx", &[0, 0]), 0x80010106);
    v.call("ole32.dll", "CoUninitialize", &[]);
    assert_eq!(v.call("ole32.dll", "CoInitializeEx", &[0, 0]), 0);
    assert_eq!(v.call("ole32.dll", "CoCreateGuid", &[DATA + 128]), 0);
    assert_eq!(v.mem.read_u8(DATA + 135).unwrap() & 0xf0, 0x40);
    assert_eq!(v.mem.read_u8(DATA + 136).unwrap() & 0xc0, 0x80);
    assert_eq!(v.call("ole32.dll", "CoCreateGuid", &[0]), 0x80004003);
}

#[test]
fn spec_ordinals_share_named_stubs_and_large_abis_read_the_full_checked_stack() {
    let mut v = Vm::new();
    let named = v.api.register("oleaut32.dll", Some("SysAllocString"), None);
    assert_eq!(v.api.register("oleaut32.dll", None, Some(2)), named);
    assert_eq!(
        v.api.entries[v.api.stub_index(named).unwrap()].name,
        "SysAllocString"
    );
    v.wide(DATA, "oleaut32.dll");
    let module = v.k("LoadLibraryW", &[DATA]);
    assert_eq!(v.k("GetProcAddress", &[module, 2]), named);
    assert_eq!(v.k("GetProcAddress", &[module, 65535]), 0);
    let addr = v.api.register("gdi32.dll", Some("CreateFontW"), None);
    let index = v.api.stub_index(addr).unwrap();
    // The last of 14 arguments straddles an uncommitted page: gateway must fault.
    v.cpu.eip = addr;
    v.cpu.set_esp(DATA + 0x10000 - 56);
    v.mem.write_u32(v.cpu.esp(), RET).unwrap();
    assert!(api::dispatch(
        index,
        &mut v.cpu,
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        1_000_000
    )
    .is_err());
    // With all arguments committed, the known stdcall ABI restores exactly 56 bytes.
    v.call("gdi32.dll", "CreateFontW", &[0; 14]);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4 + 56);
}

#[test]
fn cp932_conversion_lengths_terminators_and_errors() {
    let mut v = Vm::new();
    v.mem
        .write_bytes(DATA, &[0x93, 0xfa, 0x96, 0x7b, 0])
        .unwrap(); // 日本
    assert_eq!(
        v.k("MultiByteToWideChar", &[932, 8, DATA, u32::MAX, 0, 0]),
        3
    );
    assert_eq!(
        v.k(
            "MultiByteToWideChar",
            &[932, 8, DATA, u32::MAX, DATA + 64, 3]
        ),
        3
    );
    assert_eq!(v.mem.read_wstr(DATA + 64, 3).unwrap(), "日本");
    assert_eq!(
        v.k(
            "WideCharToMultiByte",
            &[932, 0, DATA + 64, u32::MAX, DATA + 128, 5, 0, DATA + 192]
        ),
        5
    );
    let mut bytes = [0; 5];
    v.mem.read_bytes(DATA + 128, &mut bytes).unwrap();
    assert_eq!(bytes, [0x93, 0xfa, 0x96, 0x7b, 0]);
    assert_eq!(v.mem.read_u32(DATA + 192).unwrap(), 0);
    assert_eq!(
        v.k(
            "MultiByteToWideChar",
            &[932, 8, DATA, u32::MAX, DATA + 64, 2]
        ),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 122);
    v.mem.write_bytes(DATA, &[0x81, 0]).unwrap();
    assert_eq!(
        v.k("MultiByteToWideChar", &[932, 8, DATA, 1, DATA + 64, 2]),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 1113);
    assert_eq!(
        v.k("MultiByteToWideChar", &[932, 0, DATA, 1, DATA + 64, 2]),
        1
    );
    assert_eq!(v.k("GetCPInfo", &[932, DATA]), 1);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 2);
    assert_eq!(v.mem.read_u32(DATA + 6).unwrap(), 0xfce09f81);
}

#[test]
fn x86_slist_init_and_flush_preserve_sequence_and_return_chain() {
    let mut v = Vm::new();
    v.mem.write_u64(DATA, u64::MAX).unwrap();
    v.k("InitializeSListHead", &[DATA]);
    assert_eq!(v.mem.read_u64(DATA).unwrap(), 0);
    v.mem
        .write_u64(DATA, (0xffffu64 << 48) | (2u64 << 32) | (DATA + 16) as u64)
        .unwrap();
    assert_eq!(v.k("InterlockedFlushSList", &[DATA]), DATA + 16);
    assert_eq!(v.mem.read_u64(DATA).unwrap(), 0); // sequence wrapped, depth/pointer cleared
    assert_eq!(v.k("InterlockedFlushSList", &[DATA]), 0);
    assert_eq!(v.mem.read_u64(DATA).unwrap(), 1u64 << 48);
}

#[test]
fn interlocked_compare_exchange_uses_comparand_then_exchange() {
    let mut v = Vm::new();
    v.mem.write_u32(DATA, 17).unwrap();
    assert_eq!(v.k("InterlockedCompareExchange", &[DATA, 42, 17]), 17);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 42);
    assert_eq!(v.k("InterlockedCompareExchange", &[DATA, 10, 17]), 42);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 42);
}

#[test]
fn format_message_allocation_and_local_free_share_real_heap() {
    let mut v = Vm::new();
    let count = v.k("FormatMessageW", &[0x1300, 0, 2, 0, DATA, 0, 0]);
    assert!(count > 0);
    let p = v.mem.read_u32(DATA).unwrap();
    assert_ne!(p, 0);
    assert_eq!(
        v.mem.read_wstr(p, 100).unwrap(),
        "The system cannot find the file specified.\r\n"
    );
    assert_eq!(v.k("LocalFree", &[p]), 0);
    assert_eq!(v.k("HeapValidate", &[v.state.process_heap, 0, p]), 0);
}

#[test]
fn shell_contract_keeps_pe32_argv_abi_only_folder_allocation_and_provider_specific_cleanup() {
    let mut vm = Vm::new();
    for (name, slots) in [("CommandLineToArgvW", 2), ("SHGetKnownFolderPath", 4)] {
        for dll in ["shell32.dll", "kernel32.dll", "kernelbase.dll"] {
            assert_eq!(api::native_export(dll, name), dll == "shell32.dll");
            vm.k("SetLastError", &[77]);
            vm.mem.write_u32(DATA, 0xa5a5a5a5).unwrap();
            assert_eq!(vm.call(dll, name, &vec![DATA; slots]), 0);
            if name == "SHGetKnownFolderPath" && dll == "shell32.dll" {
                assert_eq!(vm.state.last_error, 77);
                let pointer = vm.mem.read_u32(DATA).unwrap();
                assert_eq!(vm.mem.read_wstr(pointer, 4).unwrap(), "C:\\");
                assert_ne!(pointer, 0);
            } else {
                assert_eq!(vm.state.last_error, 120);
                assert_eq!(vm.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
            }
            assert_eq!(
                vm.cpu.esp(),
                STACK_BASE + STACK_SIZE - 252
                    + if dll == "shell32.dll" {
                        slots as u32 * 4
                    } else {
                        0
                    }
            );
        }
    }
}

#[test]
fn system_profile_memory_structures_and_monotonic_clocks() {
    let mut v = Vm::new();
    v.k("GetNativeSystemInfo", &[DATA]);
    assert_eq!(v.mem.read_u16(DATA).unwrap(), 0); // x86, irrespective of host
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 4096);
    assert_eq!(v.mem.read_u32(DATA + 16).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 20).unwrap(), 1);
    v.mem.write_u32(DATA + 128, 32).unwrap();
    assert_eq!(v.k("GlobalMemoryStatusEx", &[DATA + 128]), 0);
    assert_eq!(v.k("GetLastError", &[]), 87);
    v.mem.write_u32(DATA + 128, 64).unwrap();
    assert_eq!(v.k("GlobalMemoryStatusEx", &[DATA + 128]), 1);
    let available = v.mem.read_u64(DATA + 144).unwrap();
    v.k("HeapAlloc", &[v.state.process_heap, 0, 65536]);
    v.k("GlobalMemoryStatusEx", &[DATA + 128]);
    assert!(v.mem.read_u64(DATA + 144).unwrap() <= available - 65536);
    v.k("GlobalMemoryStatus", &[DATA + 256]);
    assert_eq!(v.mem.read_u32(DATA + 256).unwrap(), 32);
    assert_eq!(
        v.mem.read_u32(DATA + 264).unwrap() as u64,
        v.mem.read_u64(DATA + 136).unwrap()
    );
    v.state.start =
        std::time::Instant::now() - std::time::Duration::from_millis((1u64 << 32) + 1000);
    let ticks = v.k("GetTickCount64", &[]);
    assert_eq!(v.cpu.regs[2], 1);
    assert!((1000..2000).contains(&ticks));
    let mm = v.call("winmm.dll", "timeGetTime", &[]);
    let ordinary = v.k("GetTickCount", &[]);
    assert!(ordinary.wrapping_sub(mm) < 100);
    assert_eq!(v.call("winmm.dll", "timeGetDevCaps", &[DATA, 7]), 97);
    assert_eq!(v.call("winmm.dll", "timeGetDevCaps", &[DATA, 8]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    assert_eq!(v.call("winmm.dll", "timeBeginPeriod", &[0]), 97);
    assert_eq!(v.call("winmm.dll", "timeBeginPeriod", &[1]), 0);
    v.k("GetSystemTime", &[DATA]);
    assert!(v.mem.read_u16(DATA).unwrap() >= 2026);
    assert!((1..=12).contains(&v.mem.read_u16(DATA + 2).unwrap()));
}

#[test]
fn nls_case_mapping_lengths_buffers_and_crt_character_types() {
    let mut v = Vm::new();
    v.wide(DATA, "aZ 9日本");
    assert_eq!(
        v.k("LCMapStringEx", &[0, 0x200, DATA, u32::MAX, 0, 0, 0, 0, 0]),
        7
    );
    assert_eq!(
        v.k(
            "LCMapStringEx",
            &[0, 0x200, DATA, u32::MAX, DATA + 64, 7, 0, 0, 0]
        ),
        7
    );
    assert_eq!(v.mem.read_wstr(DATA + 64, 7).unwrap(), "AZ 9日本");
    assert_eq!(
        v.k(
            "LCMapStringW",
            &[0x411, 0x100, DATA + 64, u32::MAX, DATA + 64, 7]
        ),
        7
    );
    assert_eq!(v.mem.read_wstr(DATA + 64, 7).unwrap(), "az 9日本");
    assert_eq!(
        v.k(
            "LCMapStringEx",
            &[0, 0x200, DATA, u32::MAX, DATA + 128, 6, 0, 0, 0]
        ),
        0
    );
    assert_eq!(v.k("GetLastError", &[]), 122);
    assert_eq!(v.k("GetStringTypeW", &[1, DATA, u32::MAX, DATA + 128]), 1);
    assert_eq!(v.mem.read_u16(DATA + 128).unwrap() & 0x182, 0x182); // lower, alpha, hex 'a'
    assert_eq!(v.mem.read_u16(DATA + 132).unwrap() & 0x48, 0x48); // blank/space
    assert_eq!(v.mem.read_u16(DATA + 134).unwrap() & 0x84, 0x84); // digit/hex
    assert_eq!(v.mem.read_u16(DATA + 136).unwrap() & 0x100, 0x100); // Japanese letter
    assert_eq!(v.mem.read_u16(DATA + 140).unwrap() & 0x20, 0x20); // explicit NUL count
    assert_eq!(v.k("GetStringTypeW", &[3, DATA, 1, DATA + 128]), 0);
    assert_eq!(v.k("GetLastError", &[]), 1004);
}
