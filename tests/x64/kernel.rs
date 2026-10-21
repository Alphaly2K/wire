//! Existing behavioral tests grouped by kernel; expectations remain independent of API catalogs.

use super::*;

#[test]
fn system_profile_keeps_win64_layout_saved_frame_and_checked_output_faults() {
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        let (mut mem, mut cpu, mut runtime) = startup();
        let output = STACK + 0x100;
        let error = wire::x64::runtime::TEB + 0x68;
        mem.write_int(error, 4, 77).unwrap();
        for name in ["GetSystemInfo", "GetNativeSystemInfo"] {
            mem.write(output, &[0xa5; 56]).unwrap();
            assert_eq!(
                startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, name, &[output]),
                0
            );
            assert_eq!(mem.read_int(output, 4).unwrap(), 9);
            assert_eq!(mem.read_int(output + 4, 4).unwrap(), 4096);
            assert_eq!(mem.read_int(output + 8, 8).unwrap(), 0x10000);
            assert_eq!(mem.read_int(output + 16, 8).unwrap(), (1 << 47) - 65537);
            assert_eq!(mem.read_int(output + 24, 8).unwrap(), 1);
            for (offset, value) in [(32, 1), (36, 8664), (40, 65536), (44, 6)] {
                assert_eq!(
                    mem.read_int(output + offset, 4).unwrap(),
                    value,
                    "{name}+{offset}"
                );
            }
            assert_eq!(mem.read_int(output + 48, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
            assert_eq!(mem.read_int(error, 4).unwrap(), 77);
        }
        let sp = STACK + 0x1f88;
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "GetSystemInfo",
                &[sp]
            ),
            0
        );
        assert_eq!(cpu.rip, BASE);
        assert_eq!(mem.read_int(sp, 4).unwrap(), 9);
        for name in ["GetSystemInfo", "GlobalMemoryStatusEx"] {
            cpu.regs[4] = sp;
            mem.write_int(sp, 8, BASE).unwrap();
            cpu.regs[1] = 0;
            let before = cpu.clone();
            let calls = runtime.api_calls;
            assert!(runtime.dispatch(dll, name, &mut cpu, &mut mem).is_err());
            assert_eq!(cpu, before);
            assert_eq!(runtime.api_calls, calls);
            assert_eq!(mem.read_int(error, 4).unwrap(), 77);
        }
        mem.write(output, &[0xa5; 72]).unwrap();
        mem.write_int(output, 4, 63).unwrap();
        let mut before = [0; 64];
        mem.read(output, &mut before).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "GlobalMemoryStatusEx",
                &[output]
            ),
            0
        );
        let mut after = [0; 64];
        mem.read(output, &mut after).unwrap();
        assert_eq!(after, before);
        assert_eq!(mem.read_int(error, 4).unwrap(), 87);
        mem.write_int(output, 4, 64).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "GlobalMemoryStatusEx",
                &[output]
            ),
            1
        );
        assert_eq!(mem.read_int(output, 4).unwrap(), 64);
        assert_eq!(mem.read_int(output + 64, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
        assert_eq!(mem.read_int(error, 4).unwrap(), 87);
        assert!(!wire::x64::runtime::import_supported(
            dll,
            "GlobalMemoryStatus"
        ));
    }
}

#[test]
fn win64_isolated_identity_names_validate_lengths_domains_exports_and_output_faults() {
    let (mut m, mut cpu, mut r) = startup();
    let out = STACK + 0x100;
    let size = STACK + 0x200;
    for (dll, name, text, success_size, error) in [
        ("kernel32.dll", "GetComputerNameA", "WIRE", 4, 111),
        ("kernelbase.dll", "GetComputerNameW", "WIRE", 4, 111),
        ("kernel32.dll", "GetComputerNameExA", "wire", 4, 234),
        ("kernel32.dll", "GetComputerNameExW", "wire", 4, 234),
        ("advapi32.dll", "GetUserNameA", "wire", 5, 122),
        ("advapi32.dll", "GetUserNameW", "wire", 5, 122),
    ] {
        let args = if name.contains("Ex") {
            vec![0xaaaa00000003, out, size]
        } else {
            vec![out, size]
        };
        for n in [0, 4] {
            m.write(out, &[0xcc; 16]).unwrap();
            m.write_int(size, 4, n).unwrap();
            assert_eq!(
                startup_dll_call(&mut m, &mut cpu, &mut r, dll, name, &args),
                0
            );
            assert_eq!(m.read_int(size, 4).unwrap(), 5);
            assert_eq!(m.read_int(out, 8).unwrap(), 0xcccccccccccccccc);
            assert_eq!(
                startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
                error
            );
        }
        m.write_int(size, 4, 5).unwrap();
        assert_eq!(
            startup_dll_call(&mut m, &mut cpu, &mut r, dll, name, &args),
            1
        );
        assert_eq!(m.read_int(size, 4).unwrap(), success_size);
        let expected: Vec<u8> = if name.ends_with('W') {
            text.encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            text.bytes().chain([0]).collect()
        };
        let mut actual = vec![0; expected.len()];
        m.read(out, &mut actual).unwrap();
        assert_eq!(actual, expected);
        assert!(!wire::x64::runtime::import_supported("user32.dll", name));
        if dll == "advapi32.dll" {
            assert!(!wire::x64::runtime::import_supported("kernel32.dll", name));
        }
        r.register_provider(dll);
        m.write(out, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut m, &mut cpu, &mut r, "LoadLibraryA", &[out]);
        m.write(out, format!("{name}\0").as_bytes()).unwrap();
        let gateway = startup_call(&mut m, &mut cpu, &mut r, "GetProcAddress", &[module, out]);
        assert_eq!(r.gateways[&gateway], format!("{dll}!{name}"));
    }
    for kind in [2, 6] {
        m.write_int(size, 4, 1).unwrap();
        assert_eq!(
            startup_call(
                &mut m,
                &mut cpu,
                &mut r,
                "GetComputerNameExW",
                &[kind, out, size]
            ),
            1
        );
        assert_eq!(m.read_int(size, 4).unwrap(), 0);
        assert_eq!(m.read_int(out, 2).unwrap(), 0);
    }
    m.write_int(size, 4, 12).unwrap();
    assert_eq!(
        startup_call(
            &mut m,
            &mut cpu,
            &mut r,
            "GetComputerNameExW",
            &[8, out, size]
        ),
        0
    );
    assert_eq!(m.read_int(size, 4).unwrap(), 12);
    // Both buffer and size-pointer outputs are checked before either is changed.
    const DATA: u64 = 0x700000000;
    m.map(DATA, 8192, READ | WRITE).unwrap();
    for (buffer, count) in [(DATA + 4092, size), (out, DATA + 4094)] {
        m.write_int(count, 4, 5).unwrap();
        m.write(buffer, &[0xaa; 10]).unwrap();
        m.protect(DATA + 4096, 4096, READ).unwrap();
        cpu.regs[4] = STACK + 0x1f88;
        cpu.regs[1] = buffer;
        cpu.regs[2] = count;
        let before = cpu.clone();
        let calls = r.api_calls;
        assert!(r
            .dispatch("kernel32.dll", "GetComputerNameW", &mut cpu, &mut m)
            .is_err());
        assert_eq!(cpu.regs, before.regs);
        assert_eq!(cpu.rip, before.rip);
        assert_eq!(r.api_calls, calls);
        assert_eq!(m.read_int(count, 4).unwrap(), 5);
        let mut bytes = [0; 10];
        m.read(buffer, &mut bytes).unwrap();
        assert_eq!(bytes, [0xaa; 10]);
        m.protect(DATA + 4096, 4096, READ | WRITE).unwrap();
    }
}

#[test]
fn win64_device_discovery_has_owned_empty_sets_full_pointer_layouts_and_checked_outputs() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let guid = STACK + 0x100;
    let data = STACK + 0x200;
    startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "hid.dll",
        "HidD_GetHidGuid",
        &[guid],
    );
    let mut actual = [0; 16];
    mem.read(guid, &mut actual).unwrap();
    assert_eq!(
        actual,
        [0xb2, 0x55, 0x1e, 0x4d, 0x6f, 0xf1, 0xcf, 0x11, 0x88, 0xcb, 0, 0x11, 0x11, 0, 0, 0x30]
    );
    let set = startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "setupapi.dll",
        "SetupDiGetClassDevsA",
        &[guid, 0, 0, 0xabcdef0100000012],
    );
    assert!(set > u32::MAX as u64 && set != u64::MAX);
    mem.write(data, &[0xa5; 32]).unwrap();
    mem.write_int(data, 4, 32).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "setupapi.dll",
            "SetupDiEnumDeviceInterfaces",
            &[set, 0, guid, 0, data]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        259
    );
    let mut record = [0; 32];
    mem.read(data, &mut record).unwrap();
    assert_eq!(record[..4], 32u32.to_le_bytes());
    assert_eq!(record[4..], [0; 28]);
    mem.write_int(data + 24, 8, 0xfedcba9876543210).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "setupapi.dll",
            "SetupDiEnumDeviceInfo",
            &[set, 0, data]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        259
    );
    assert_eq!(mem.read_int(data + 24, 8).unwrap(), 0xfedcba9876543210);
    let required = data + 64;
    mem.write_int(required, 4, 0x55555555).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "setupapi.dll",
            "SetupDiGetDeviceInterfaceDetailW",
            &[set, data, 0, 0, required, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        87
    );
    assert_eq!(mem.read_int(required, 4).unwrap(), 0x55555555);
    let fault = STACK + 8184;
    mem.write(fault, &[0xaa; 8]).unwrap();
    cpu.regs[4] = STACK + 0x1f88;
    cpu.regs[1] = fault;
    let before = cpu.clone();
    assert!(runtime
        .dispatch("hid.dll", "HidD_GetHidGuid", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(mem.read_int(fault, 8).unwrap(), 0xaaaaaaaaaaaaaaaa);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "setupapi.dll",
            "SetupDiDestroyDeviceInfoList",
            &[set]
        ),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "setupapi.dll",
            "SetupDiEnumDeviceInfo",
            &[set, 0, data]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        6
    );
    let another = startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "setupapi.dll",
        "SetupDiGetClassDevsW",
        &[0, 0, 0, 4],
    );
    assert!(another > set);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "setupapi.dll",
            "SetupDiGetClassDevsW",
            &[0, 0, 0, 2]
        ),
        u64::MAX
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        87
    );
}

#[test]
fn win64_hid_absence_preserves_outputs_status_and_dll_export_ownership() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x100;
    let text = STACK + 0x200;
    mem.write(output, &[0xaa; 128]).unwrap();
    for (name, args) in [
        ("HidD_GetAttributes", vec![0x7777777700000001, output]),
        ("HidD_GetPreparsedData", vec![0x7777777700000001, output]),
        (
            "HidD_GetManufacturerString",
            vec![0x7777777700000001, output, 128],
        ),
        (
            "HidD_GetProductString",
            vec![0x7777777700000001, output, 128],
        ),
        (
            "HidD_GetSerialNumberString",
            vec![0x7777777700000001, output, 128],
        ),
    ] {
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut runtime, "hid.dll", name, &args),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            6
        );
    }
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetLastError", &[0x1234]);
    for (name, args) in [
        ("HidP_GetCaps", vec![0, output]),
        ("HidP_GetButtonCaps", vec![0, output, output + 8, 0]),
        ("HidP_GetValueCaps", vec![0, output, output + 8, 0]),
        (
            "HidP_GetData",
            vec![0, output, output + 8, 0, output + 16, 16],
        ),
        ("HidP_SetUsageValue", vec![0, 1, 0, 1, 1, 0, output, 16]),
        (
            "HidP_SetUsages",
            vec![0, 1, 0, output, output + 8, 0, output + 16, 16],
        ),
    ] {
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut runtime, "hid.dll", name, &args),
            0xc0110001
        );
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "hid.dll",
            "HidP_MaxDataListLength",
            &[0, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0x1234
    );
    let mut bytes = [0; 128];
    mem.read(output, &mut bytes).unwrap();
    assert_eq!(bytes, [0xaa; 128]);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "hid.dll",
            "HidD_FreePreparsedData",
            &[0]
        ),
        1
    );
    mem.write(text, b"hid.dll\0").unwrap();
    let module = startup_call(&mut mem, &mut cpu, &mut runtime, "LoadLibraryA", &[text]);
    assert_ne!(module, 0);
    mem.write(text, b"HidD_GetHidGuid\0").unwrap();
    let gateway = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetProcAddress",
        &[module, text],
    );
    assert_eq!(
        runtime.gateways.get(&gateway).unwrap(),
        "hid.dll!HidD_GetHidGuid"
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[wire::x64::runtime::TEB + 0x10000, text]
        ),
        0
    );
    assert!(!wire::x64::runtime::import_supported(
        "kernel32.dll",
        "HidD_GetHidGuid"
    ));
    assert!(!wire::x64::runtime::import_supported(
        "hid.dll",
        "SetupDiEnumDeviceInterfaces"
    ));
}

#[test]
fn win64_extended_codepage_info_has_windows_layout_and_checked_full_outputs() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x400;
    for (name, size, cp, maximum, unicode_default) in [
        ("GetCPInfoExW", 544, 0, 2, 0x30fb),
        ("GetCPInfoExA", 284, 1252, 1, 0x3f),
        ("GetCPInfoExW", 544, 65001, 4, 0xfffd),
    ] {
        mem.write(output, &[0xaa; 560]).unwrap();
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, name, &[cp, 0, output]),
            1
        );
        assert_eq!(mem.read_int(output, 4).unwrap(), maximum);
        assert_eq!(mem.read_int(output + 18, 2).unwrap(), unicode_default);
        assert_eq!(
            mem.read_int(output + 20, 4).unwrap(),
            if cp == 0 { 932 } else { cp }
        );
        assert_ne!(mem.read_int(output + 24, 1).unwrap(), 0);
        assert_eq!(mem.read_int(output + size - 1, 1).unwrap(), 0);
        assert_eq!(mem.read_int(output + size, 1).unwrap(), 0xaa);
    }
    mem.write(output, &[0xaa; 560]).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetCPInfoExW",
            &[0, 1, output]
        ),
        0
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 0xaaaaaaaaaaaaaaaa);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        1004
    );
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = 0;
    cpu.regs[2] = 0;
    cpu.regs[8] = STACK + 8192 - 16;
    mem.write(cpu.regs[8], &[0xaa; 16]).unwrap();
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("kernel32.dll", "GetCPInfoExW", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(
        mem.read_int(STACK + 8192 - 16, 8).unwrap(),
        0xaaaaaaaaaaaaaaaa
    );
}

#[test]
fn win64_small_heap_blocks_pack_reuse_and_reallocate_without_overlapping_live_data() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let heap = startup_call(&mut mem, &mut cpu, &mut runtime, "GetProcessHeap", &[]);
    let committed = |m: &Memory| {
        let s = m.memory_status();
        u64::from_le_bytes(s[8..16].try_into().unwrap())
            - u64::from_le_bytes(s[16..24].try_into().unwrap())
    };
    let baseline = committed(&mem);
    let mut blocks = Vec::new();
    for n in 0..4096u64 {
        let p = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "HeapAlloc",
            &[heap, 8, 28],
        );
        assert_ne!(p, 0);
        assert_eq!(p % 16, 0);
        assert_eq!(mem.read_int(p, 8).unwrap(), 0);
        mem.write_int(p, 8, n + 1).unwrap();
        blocks.push(p);
    }
    assert!(committed(&mem) - baseline <= 128 * 1024);
    for (n, &p) in blocks.iter().enumerate() {
        assert_eq!(mem.read_int(p, 8).unwrap(), n as u64 + 1);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "HeapSize", &[heap, 0, p]),
            28
        );
    }
    let p = blocks.pop().unwrap();
    startup_call(&mut mem, &mut cpu, &mut runtime, "HeapFree", &[heap, 0, p]);
    let reused = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "HeapAlloc",
        &[heap, 8, 28],
    );
    assert_eq!(reused, p);
    assert_eq!(mem.read_int(reused, 8).unwrap(), 0);
    mem.write_int(reused, 8, 0x123456789abcdef0).unwrap();
    let grown = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "HeapReAlloc",
        &[heap, 8, reused, 100],
    );
    assert_eq!(mem.read_int(grown, 8).unwrap(), 0x123456789abcdef0);
    assert_eq!(mem.read_int(grown + 92, 8).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "HeapSize",
            &[heap, 0, grown]
        ),
        100
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "HeapReAlloc",
            &[heap, 0, grown, 20_000_000]
        ),
        0
    );
    assert_eq!(mem.read_int(grown, 8).unwrap(), 0x123456789abcdef0);
    for p in blocks.into_iter().chain(Some(grown)) {
        startup_call(&mut mem, &mut cpu, &mut runtime, "HeapFree", &[heap, 0, p]);
    }
    assert!(committed(&mem) - baseline <= 128 * 1024); // one empty slab retained per used bucket
}

#[test]
fn win64_heap_in_place_reallocation_preserves_address_or_leaves_block_unchanged() {
    let (mut mem, mut cpu, mut rt) = startup();
    let heap = startup_call(&mut mem, &mut cpu, &mut rt, "GetProcessHeap", &[]);
    for (initial, within, beyond) in [(17, 32, 33), (0x417, 0x800, 0x817), (4097, 8192, 8193)] {
        let p = startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "HeapAlloc",
            &[heap, 0, initial],
        );
        mem.write(p, &vec![0xa5; initial as usize]).unwrap();
        startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
        let pages = mem.memory_status();
        for size in [beyond, u64::MAX] {
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "HeapReAlloc",
                    &[heap, 0x10, p, size]
                ),
                0
            );
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut rt, "HeapSize", &[heap, 0, p]),
                initial
            );
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
                123
            );
            assert_eq!(mem.memory_status(), pages);
            assert_eq!(mem.read_int(p, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
        }
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "HeapReAlloc",
                &[heap, 0x19, p, within]
            ),
            p
        );
        let mut bytes = vec![0; within as usize];
        mem.read(p, &mut bytes).unwrap();
        assert!(bytes[..initial as usize].iter().all(|&b| b == 0xa5));
        assert!(bytes[initial as usize..].iter().all(|&b| b == 0));
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "HeapReAlloc",
                &[heap, 0x10, p, 0]
            ),
            p
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "HeapSize", &[heap, 0, p]),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "HeapFree", &[heap, 0, p]),
            1
        );
    }
}

#[test]
fn win64_system_times_use_real_cpu_counters_and_preflight_all_outputs() {
    let (mut mem, mut cpu, mut runtime) = startup();
    const DATA: u64 = 0x6d0000000;
    mem.map(DATA, 4096, READ | WRITE).unwrap();
    assert!(wire::x64::runtime::import_supported(
        "kernel32.dll",
        "GetSystemTimes"
    ));
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetSystemTimes",
            &[DATA, DATA + 8, DATA + 16]
        ),
        1
    );
    let first: [u64; 3] = std::array::from_fn(|n| mem.read_int(DATA + n as u64 * 8, 8).unwrap());
    assert!(first[1] >= first[0]);
    assert!(first[1] > 0 && first[2] > 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetSystemTimes",
            &[DATA, DATA + 8, DATA + 16]
        ),
        1
    );
    for (n, before) in first.into_iter().enumerate() {
        assert!(mem.read_int(DATA + n as u64 * 8, 8).unwrap() >= before);
    }
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetSystemTimes",
            &[0, 0, 0]
        ),
        1
    );
    mem.write(DATA, &[0xa5; 24]).unwrap();
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = DATA;
    cpu.regs[2] = DATA + 8;
    cpu.regs[8] = DATA + 4096 - 4;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("kernel32.dll", "GetSystemTimes", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    let mut output = [0; 24];
    mem.read(DATA, &mut output).unwrap();
    assert_eq!(output, [0xa5; 24]);
}

#[test]
fn win64_registry_owns_high_handles_roundtrips_cp932_and_keeps_errors_outputs_and_access_real() {
    let (mut mem, mut cpu, mut runtime) = startup();
    const HKCU: u64 = 0xffffffff80000001;
    let path = STACK + 0x200;
    let value = STACK + 0x300;
    let output = STACK + 0x500;
    let data = STACK + 0x600;
    macro_rules! call {
        ($name:expr, $args:expr) => {
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "advapi32.dll",
                $name,
                $args,
            )
        };
    }
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetLastError",
        &[0xabcdef],
    );
    mem.write(
        path,
        &"Software\\Wire\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    mem.write(output, &[0xcc; 32]).unwrap();
    assert_eq!(
        call!(
            "RegCreateKeyExW",
            &[HKCU, path, 0, 0, 0, 0xf003f, 0, output, output + 8]
        ),
        0
    );
    let handle = mem.read_int(output, 8).unwrap();
    assert!(handle > u32::MAX as u64);
    assert_eq!(mem.read_int(output + 8, 4).unwrap(), 1);
    assert_eq!(
        call!(
            "RegCreateKeyExW",
            &[HKCU, path, 0, 0, 0, 0x20019, 0, output + 16, output + 8]
        ),
        0
    );
    assert_eq!(mem.read_int(output + 8, 4).unwrap(), 2);
    let readonly = mem.read_int(output + 16, 8).unwrap();
    mem.write(value, b"Text\0").unwrap();
    let cp932 = [0x82, 0xa0, 0, 0x41, 0, 0]; // MULTI_SZ preserves embedded NULs.
    mem.write(data, &cp932).unwrap();
    assert_eq!(
        call!(
            "RegSetValueExA",
            &[handle, value, 0, 7, data, cp932.len() as u64]
        ),
        0
    );
    assert_eq!(
        call!(
            "RegSetValueExA",
            &[readonly, value, 0, 7, data, cp932.len() as u64]
        ),
        5
    );
    mem.write(
        value,
        &"tExT\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    mem.write_int(output + 8, 4, 0).unwrap();
    assert_eq!(
        call!(
            "RegQueryValueExW",
            &[handle, value, 0, output + 12, 0, output + 8]
        ),
        0
    );
    assert_eq!(mem.read_int(output + 8, 4).unwrap(), 10);
    assert_eq!(mem.read_int(output + 12, 4).unwrap(), 7);
    mem.write(data, &[0xaa; 16]).unwrap();
    mem.write_int(output + 8, 4, 2).unwrap();
    assert_eq!(
        call!(
            "RegQueryValueExW",
            &[readonly, value, 0, output + 12, data, output + 8]
        ),
        234
    );
    assert_eq!(mem.read_int(output + 8, 4).unwrap(), 10);
    assert_eq!(mem.read_int(data, 8).unwrap(), 0xaaaaaaaaaaaaaaaa);
    assert_eq!(
        call!(
            "RegQueryValueExW",
            &[readonly, value, 0, output + 12, data, output + 8]
        ),
        0
    );
    let mut wide = [0; 10];
    mem.read(data, &mut wide).unwrap();
    assert_eq!(wide, [0x42, 0x30, 0, 0, 0x41, 0, 0, 0, 0, 0]);
    mem.write(value, b"TEXT\0").unwrap();
    mem.write_int(output + 8, 4, 16).unwrap();
    assert_eq!(
        call!("RegQueryValueExA", &[handle, value, 0, 0, data, output + 8]),
        0
    );
    let mut ansi = [0; 6];
    mem.read(data, &mut ansi).unwrap();
    assert_eq!(ansi, cp932);
    assert_eq!(call!("RegCloseKey", &[handle]), 0);
    assert_eq!(call!("RegCloseKey", &[handle]), 6);
    mem.write(path, b"software\\WIRE\0").unwrap();
    assert_eq!(call!("RegOpenKeyExA", &[HKCU, path, 0, 3, output]), 0);
    let reopened = mem.read_int(output, 8).unwrap();
    assert_ne!(reopened, handle);
    assert_eq!(
        call!(
            "RegQueryValueExA",
            &[reopened, value, 0, 0, data, output + 8]
        ),
        0
    );
    // An unknown system installation is absent, not fabricated from a successful open.
    assert_eq!(
        call!(
            "RegOpenKeyExA",
            &[0xffffffff80000002, path, 0, 0x20019, output]
        ),
        2
    );
    assert_eq!(
        call!("RegCreateKeyA", &[0xffffffff80000002, path, output]),
        5
    );
    assert_eq!(
        call!("RegOpenKeyExA", &[HKCU, path, 0, 0x20219, output]),
        50
    );
    assert_eq!(call!("RegDeleteValueA", &[reopened, value]), 0);
    assert_eq!(
        call!(
            "RegQueryValueExA",
            &[reopened, value, 0, 0, data, output + 8]
        ),
        2
    );
    assert_eq!(call!("RegDeleteValueA", &[reopened, value]), 2);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0xabcdef
    );
    // Bad disposition output cannot create the key or write the handle output.
    mem.write(path, b"Software\\NotCreated\0").unwrap();
    mem.write_int(output, 8, 0xdeadbeefcafebabe).unwrap();
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    let args = [HKCU, path, 0, 0, 0, 3, 0, output, STACK + 8192 - 1];
    for (i, arg) in args.into_iter().enumerate() {
        if i < 4 {
            cpu.regs[[1, 2, 8, 9][i]] = arg;
        } else {
            mem.write_int(cpu.regs[4] + 40 + (i as u64 - 4) * 8, 8, arg)
                .unwrap();
        }
    }
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("advapi32.dll", "RegCreateKeyExA", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(mem.read_int(output, 8).unwrap(), 0xdeadbeefcafebabe);
    assert_eq!(call!("RegOpenKeyExA", &[HKCU, path, 0, 1, output]), 2);
}

#[test]
fn win64_version_profile_queries_verify_hierarchically_and_keep_ntstatus_and_output_faults() {
    let (mut mem, mut cpu, mut runtime) = startup();
    runtime.install_gateways(Default::default());
    let output = STACK + 0x200;
    for (dll, name, size) in [
        ("ntdll.dll", "RtlGetVersion", 284),
        ("kernel32.dll", "GetVersionExW", 276),
        ("kernel32.dll", "GetVersionExA", 156),
    ] {
        mem.write(output, &[0xcc; 284]).unwrap();
        mem.write_int(output, 4, size).unwrap();
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, name, &[output]),
            (dll == "kernel32.dll") as u64
        );
        assert_eq!(mem.read_int(output + 4, 4).unwrap(), 10);
        assert_eq!(mem.read_int(output + 8, 4).unwrap(), 0);
        assert_eq!(mem.read_int(output + 12, 4).unwrap(), 19045);
        if size != 276 {
            assert_eq!(mem.read_int(output + size - 4, 2).unwrap(), 0x100);
            assert_eq!(mem.read_int(output + size - 2, 1).unwrap(), 1);
        }
    }
    mem.write_int(output, 4, 284).unwrap();
    startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "ntdll.dll",
        "RtlGetVersion",
        &[output],
    );
    let major = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "VerSetConditionMask",
        &[0x8000000000000000, 2, 3],
    );
    let both = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "VerSetConditionMask",
        &[major, 1, 3],
    );
    assert_eq!(both, 0x800000000000001b);
    for (major, minor, expected) in [
        (9, 99, 0),
        (10, 0, 0),
        (10, 1, 0xc0000059),
        (11, 0, 0xc0000059),
    ] {
        mem.write_int(output + 4, 4, major).unwrap();
        mem.write_int(output + 8, 4, minor).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "ntdll.dll",
                "RtlVerifyVersionInfo",
                &[output, 3, both]
            ),
            expected
        );
    }
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetLastError", &[777]);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "RtlVerifyVersionInfo",
            &[output, 3, both]
        ),
        0xc0000059
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        777
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "VerifyVersionInfoW",
            &[output, 3, both]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        1150
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "RtlVerifyVersionInfo",
            &[output, 0, both]
        ),
        0xc000000d
    );
    mem.write_int(output + 280, 2, 0x100).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "RtlVerifyVersionInfo",
            &[output, 0x40, 6 << 18]
        ),
        0
    );
    mem.write_int(output + 280, 2, 1).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ntdll.dll",
            "RtlVerifyVersionInfo",
            &[output, 0x40, 7 << 18]
        ),
        0xc0000059
    );

    mem.write(output, b"ntdll\0").unwrap();
    let ntdll = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetModuleHandleA",
        &[output],
    );
    assert_ne!(ntdll, 0);
    mem.write(output + 32, b"RtlVerifyVersionInfo\0").unwrap();
    assert_ne!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[ntdll, output + 32]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "FreeLibrary", &[ntdll]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetModuleHandleA",
            &[output]
        ),
        ntdll
    );
    let bad = STACK + 4096 - 100;
    mem.write(bad, &[0xcc; 100]).unwrap();
    mem.write_int(bad, 4, 276).unwrap();
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    cpu.regs[4] = STACK + 0x188;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = bad;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("ntdll.dll", "RtlGetVersion", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(mem.read_int(bad + 4, 8).unwrap(), 0xcccccccccccccccc);
}

#[test]
fn win64_token_sid_layout_access_and_local_reallocation_preserve_failure_state() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let out = STACK + 0x100;
    let used = STACK + 0x180;
    let info = STACK + 0x200;
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "OpenProcessToken",
            &[u64::MAX, 8, out]
        ),
        1
    );
    let handle = mem.read_int(out, 8).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "GetTokenInformation",
            &[handle, 25, 0, 0, used]
        ),
        0
    );
    assert_eq!(mem.read_int(used, 4).unwrap(), 28);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        122
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "GetTokenInformation",
            &[handle, 25, info, 28, used]
        ),
        1
    );
    let sid = mem.read_int(info, 8).unwrap();
    assert_eq!(sid, info + 16);
    assert_eq!(mem.read_int(info + 8, 4).unwrap(), 0x60);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "GetSidSubAuthority",
            &[sid, 0]
        ),
        sid + 8
    );
    assert_eq!(mem.read_int(sid + 8, 4).unwrap(), 8192);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "GetLengthSid",
            &[sid]
        ),
        12
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "CopySid",
            &[12, info + 64, sid]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "EqualSid",
            &[sid, info + 64]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "IsValidSid",
            &[1 << 48]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        1337
    );
    mem.write_int(out, 8, 0xabcdef).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "OpenProcessToken",
            &[u64::MAX, 0x20, out]
        ),
        0
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xabcdef);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "OpenProcessToken",
            &[u64::MAX, 0, out]
        ),
        1
    );
    let inaccessible = mem.read_int(out, 8).unwrap();
    mem.write_int(used, 4, 0x1234).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "advapi32.dll",
            "GetTokenInformation",
            &[inaccessible, 20, info, 28, used]
        ),
        0
    );
    assert_eq!(mem.read_int(used, 4).unwrap(), 0x1234);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        5
    );
    // A valid token with a bad output range leaves the return frame and required-size output untouched.
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = handle;
    cpu.regs[2] = 25;
    cpu.regs[8] = STACK + 8192 - 16;
    cpu.regs[9] = 28;
    mem.write_int(cpu.regs[4] + 40, 8, used).unwrap();
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("advapi32.dll", "GetTokenInformation", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(mem.read_int(used, 4).unwrap(), 0x1234);
    let allocation = startup_call(&mut mem, &mut cpu, &mut runtime, "LocalAlloc", &[0x40, 8]);
    mem.write_int(allocation, 8, 0x12345678abcdef01).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "LocalReAlloc",
            &[allocation, 5000, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "LocalSize", &[allocation]),
        8
    );
    assert_eq!(mem.read_int(allocation, 8).unwrap(), 0x12345678abcdef01);
    let moved = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "LocalReAlloc",
        &[allocation, 5000, 0x42],
    );
    assert_ne!(moved, allocation);
    assert!(mem.read_int(allocation, 1).is_err());
    assert_eq!(mem.read_int(moved, 8).unwrap(), 0x12345678abcdef01);
    assert_eq!(mem.read_int(moved + 8, 8).unwrap(), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "LocalSize", &[moved]),
        5000
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "LocalFree", &[moved]),
        0
    );
    assert!(mem.read_int(moved, 1).is_err());
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[handle]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CloseHandle",
            &[inaccessible]
        ),
        1
    );
}

#[test]
fn win64_startup_layout_time_tls_and_recursive_locks_are_real_checked_objects() {
    use wire::x64::runtime::{PEB, TEB};
    let (mut mem, mut cpu, mut runtime) = startup();
    assert_eq!(mem.read_int(TEB + 0x30, 8).unwrap(), TEB);
    assert_eq!(mem.read_int(TEB + 0x60, 8).unwrap(), PEB);
    assert_eq!(mem.read_int(PEB + 0x10, 8).unwrap(), BASE);
    let output = STACK + 0x100;
    let flags = cpu.rflags;
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetSystemTimeAsFileTime",
        &[output],
    );
    assert!(mem.read_int(output, 8).unwrap() > 116444736000000000);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "QueryPerformanceFrequency",
        &[output],
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 10_000_000);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetStartupInfoW",
        &[output],
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 104);
    assert_eq!(mem.read_int(output + 96, 8).unwrap(), 0);
    let slot = startup_call(&mut mem, &mut cpu, &mut runtime, "TlsAlloc", &[]);
    let value = 0x123456789abcdef0;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TlsSetValue",
            &[slot, value]
        ),
        1
    );
    assert_eq!(mem.read_int(TEB + 0x1480 + slot * 8, 8).unwrap(), value);
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetLastError", &[123]);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "TlsGetValue", &[slot]),
        value
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0
    );
    assert_eq!(mem.read_int(PEB + 0x10, 8).unwrap(), BASE); // TEB TLS slots must not overlap PEB.
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "InitializeCriticalSectionAndSpinCount",
        &[output, 4000],
    );
    for _ in 0..2 {
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "EnterCriticalSection",
            &[output],
        );
    }
    assert_eq!(mem.read_int(output + 12, 4).unwrap(), 2);
    assert_eq!(mem.read_int(output + 16, 8).unwrap(), 1);
    for _ in 0..2 {
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "LeaveCriticalSection",
            &[output],
        );
    }
    assert_eq!(mem.read_int(output + 8, 4).unwrap(), u32::MAX as u64);
    assert_eq!(mem.read_int(output + 16, 8).unwrap(), 0);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "InitializeCriticalSection",
        &[output],
    );
    assert_eq!(mem.read_int(output + 32, 8).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetCriticalSectionSpinCount",
            &[output, 777]
        ),
        0
    );
    assert_eq!(mem.read_int(output + 32, 8).unwrap(), 777);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TryEnterCriticalSection",
            &[output]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TryEnterCriticalSection",
            &[output]
        ),
        1
    );
    assert_eq!(mem.read_int(output + 12, 4).unwrap(), 2);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "LeaveCriticalSection",
        &[output],
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "LeaveCriticalSection",
        &[output],
    );
    // TryEnter never blocks, steals another thread's lock, or changes the LastError value.
    mem.write_int(output + 16, 8, 2).unwrap();
    mem.write_int(output + 12, 4, 1).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TryEnterCriticalSection",
            &[output]
        ),
        0
    );
    assert_eq!(mem.read_int(output + 16, 8).unwrap(), 2);
    assert_eq!(cpu.rflags, flags);
}

#[test]
fn win64_system_layout_and_host_calendar_have_complete_checked_outputs() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let pointer = STACK + 0x100;
    for name in ["GetSystemInfo", "GetNativeSystemInfo"] {
        startup_call(&mut mem, &mut cpu, &mut runtime, name, &[pointer]);
        assert_eq!(mem.read_int(pointer, 2).unwrap(), 9);
        assert_eq!(mem.read_int(pointer + 4, 4).unwrap(), 4096);
        assert_eq!(mem.read_int(pointer + 8, 8).unwrap(), 0x10000);
        assert_eq!(mem.read_int(pointer + 24, 8).unwrap(), 1);
        assert_eq!(mem.read_int(pointer + 32, 4).unwrap(), 1);
        assert_eq!(mem.read_int(pointer + 40, 4).unwrap(), 65536);
    }
    for name in ["GetSystemTime", "GetLocalTime"] {
        startup_call(&mut mem, &mut cpu, &mut runtime, name, &[pointer]);
        assert!((2020..2100).contains(&mem.read_int(pointer, 2).unwrap()));
        assert!((1..=12).contains(&mem.read_int(pointer + 2, 2).unwrap()));
        assert!(mem.read_int(pointer + 4, 2).unwrap() <= 6);
        assert!((1..=31).contains(&mem.read_int(pointer + 6, 2).unwrap()));
        assert!(mem.read_int(pointer + 8, 2).unwrap() <= 23);
        assert!(mem.read_int(pointer + 10, 2).unwrap() <= 59);
    }
}

#[test]
fn win64_calendar_roundtrips_epoch_leap_centuries_and_rejects_invalid_dates() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let source = STACK + 0x100;
    let ticks = STACK + 0x200;
    let result = STACK + 0x300;
    for (fields, expected) in [
        ([1601u16, 1, 0, 1, 0, 0, 0, 0], Some(0)),
        ([1970, 1, 0, 1, 0, 0, 0, 0], Some(116444736000000000)),
        ([2000, 1, 0, 1, 0, 0, 0, 0], Some(125911584000000000)),
        ([2000, 2, 0, 29, 23, 59, 59, 999], None),
        ([30827, 12, 0, 31, 23, 59, 59, 999], None),
    ] {
        let bytes: Vec<u8> = fields.into_iter().flat_map(u16::to_le_bytes).collect();
        mem.write(source, &bytes).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "SystemTimeToFileTime",
                &[source, ticks]
            ),
            1
        );
        if let Some(expected) = expected {
            assert_eq!(mem.read_int(ticks, 8).unwrap(), expected);
        }
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "FileTimeToSystemTime",
                &[ticks, result]
            ),
            1
        );
        for n in [0, 1, 3, 4, 5, 6, 7] {
            assert_eq!(
                mem.read_int(result + n * 2, 2).unwrap(),
                fields[n as usize] as u64
            );
        }
        assert!(mem.read_int(result + 4, 2).unwrap() <= 6);
    }
    for fields in [
        [1900u16, 2, 0, 29, 0, 0, 0, 0],
        [2000, 2, 0, 30, 0, 0, 0, 0],
        [1600, 1, 0, 1, 0, 0, 0, 0],
        [2000, 1, 0, 1, 24, 0, 0, 0],
    ] {
        mem.write(
            source,
            &fields
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        mem.write_int(ticks, 8, 0x11223344).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "SystemTimeToFileTime",
                &[source, ticks]
            ),
            0
        );
        assert_eq!(mem.read_int(ticks, 8).unwrap(), 0x11223344);
    }
    mem.write_int(ticks, 8, 1 << 63).unwrap();
    mem.write(result, &[0x77; 16]).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "FileTimeToSystemTime",
            &[ticks, result]
        ),
        0
    );
    let mut bytes = [0; 16];
    mem.read(result, &mut bytes).unwrap();
    assert_eq!(bytes, [0x77; 16]);
}

#[test]
fn win64_timezone_conversion_obeys_dst_boundaries_gaps_and_checked_aliasing() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let zone = STACK + 0x100;
    let input = STACK + 0x300;
    let output = STACK + 0x400;
    fn systemtime(fields: [u16; 8]) -> Vec<u8> {
        fields.into_iter().flat_map(u16::to_le_bytes).collect()
    }
    fn read_fields(mem: &Memory, address: u64) -> [u16; 8] {
        std::array::from_fn(|n| mem.read_int(address + n as u64 * 2, 2).unwrap() as u16)
    }
    // Query the actual host zone, then check that the supplied Windows structure
    // reproduces default-zone conversion at the current instant.
    let mode = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetTimeZoneInformation",
        &[zone],
    );
    assert!(mode <= 2);
    startup_call(&mut mem, &mut cpu, &mut runtime, "GetSystemTime", &[input]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SystemTimeToTzSpecificLocalTime",
            &[0, input, output]
        ),
        1
    );
    let host = read_fields(&mem, output);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SystemTimeToTzSpecificLocalTime",
            &[zone, input, output]
        ),
        1
    );
    assert_eq!(read_fields(&mem, output), host);
    // US Eastern: second Sunday in March at 02:00, first Sunday in November at 02:00.
    let mut tz = [0; 172];
    put32(&mut tz, 0, 300);
    put32(&mut tz, 168, (-60i32) as u32);
    tz[68..84].copy_from_slice(&systemtime([0, 11, 0, 1, 2, 0, 0, 0]));
    tz[152..168].copy_from_slice(&systemtime([0, 3, 0, 2, 2, 0, 0, 0]));
    mem.write(zone, &tz).unwrap();
    for (utc, local) in [
        (
            [2024, 3, 0, 10, 6, 59, 59, 999],
            [2024, 3, 0, 10, 1, 59, 59, 999],
        ),
        ([2024, 3, 0, 10, 7, 0, 0, 0], [2024, 3, 0, 10, 3, 0, 0, 0]),
        (
            [2024, 11, 0, 3, 5, 59, 59, 0],
            [2024, 11, 0, 3, 1, 59, 59, 0],
        ),
        ([2024, 11, 0, 3, 6, 0, 0, 0], [2024, 11, 0, 3, 1, 0, 0, 0]),
    ] {
        mem.write(input, &systemtime(utc)).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "SystemTimeToTzSpecificLocalTime",
                &[zone, input, output]
            ),
            1
        );
        let result = read_fields(&mem, output);
        for n in [0, 1, 3, 4, 5, 6, 7] {
            assert_eq!(result[n], local[n]);
        }
    }
    // Resolve overlap to standard time and reject nonexistent spring-forward time.
    for (local, expected) in [
        (
            [2024, 11, 0, 3, 1, 30, 0, 123],
            Some([2024, 11, 0, 3, 6, 30, 0, 123]),
        ),
        ([2024, 3, 0, 10, 2, 30, 0, 0], None),
    ] {
        mem.write(input, &systemtime(local)).unwrap();
        mem.write(output, &[0xa5; 16]).unwrap();
        let result = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TzSpecificLocalTimeToSystemTime",
            &[zone, input, output],
        );
        if let Some(expected) = expected {
            assert_eq!(result, 1);
            let fields = read_fields(&mem, output);
            for n in [0, 1, 3, 4, 5, 6, 7] {
                assert_eq!(fields[n], expected[n]);
            }
        } else {
            assert_eq!(result, 0);
            assert_eq!(read_fields(&mem, output), [0xa5a5; 8]);
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
                87
            );
        }
    }
    // Southern hemisphere crosses January and uses the last Sunday of a short month.
    put32(&mut tz, 0, (-600i32) as u32);
    tz[68..84].copy_from_slice(&systemtime([0, 2, 0, 5, 3, 0, 0, 0]));
    tz[152..168].copy_from_slice(&systemtime([0, 10, 0, 1, 2, 0, 0, 0]));
    mem.write(zone, &tz).unwrap();
    for (utc, local) in [
        ([2024, 1, 0, 1, 0, 0, 0, 0], [2024, 1, 0, 1, 11, 0, 0, 0]),
        (
            [2024, 2, 0, 24, 15, 59, 59, 0],
            [2024, 2, 0, 25, 2, 59, 59, 0],
        ),
        ([2024, 2, 0, 24, 16, 0, 0, 0], [2024, 2, 0, 25, 2, 0, 0, 0]),
        ([2024, 7, 0, 1, 0, 0, 0, 0], [2024, 7, 0, 1, 10, 0, 0, 0]),
    ] {
        mem.write(input, &systemtime(utc)).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "SystemTimeToTzSpecificLocalTime",
                &[zone, input, input]
            ),
            1
        ); // aliases input
        let result = read_fields(&mem, input);
        for n in [0, 1, 3, 4, 5, 6, 7] {
            assert_eq!(result[n], local[n]);
        }
    }
    // A fixed zone ignores standard/daylight extra biases and preserves a year crossing.
    tz[68..84].fill(0);
    tz[152..168].fill(0);
    put32(&mut tz, 0, (-480i32) as u32);
    put32(&mut tz, 84, 1234);
    mem.write(zone, &tz).unwrap();
    mem.write(input, &systemtime([2023, 12, 0, 31, 20, 0, 0, 456]))
        .unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SystemTimeToTzSpecificLocalTime",
            &[zone, input, output]
        ),
        1
    );
    assert_eq!(read_fields(&mem, output), [2024, 1, 1, 1, 4, 0, 0, 456]);
    // A null timezone consults host tzdata; an ordinary midday value roundtrips.
    let fields = [2024, 7, 0, 1, 12, 13, 14, 789];
    mem.write(input, &systemtime(fields)).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SystemTimeToTzSpecificLocalTime",
            &[0, input, output]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TzSpecificLocalTimeToSystemTime",
            &[0, output, input]
        ),
        1
    );
    let result = read_fields(&mem, input);
    for n in [0, 1, 3, 4, 5, 6, 7] {
        assert_eq!(result[n], fields[n]);
    }
    cpu.regs[1] = zone;
    cpu.regs[2] = input;
    cpu.regs[8] = STACK + 8192 - 8;
    mem.write_int(cpu.regs[8], 8, 0xa5a5a5a5a5a5a5a5).unwrap();
    let before = cpu.clone();
    assert!(runtime
        .dispatch(
            "kernel32.dll",
            "SystemTimeToTzSpecificLocalTime",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(mem.read_int(cpu.regs[8], 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
}

#[test]
fn win64_child_startup_attributes_query_size_and_missing_executable_do_not_fake_process() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let size = STACK + 0x100;
    let list = STACK + 0x200;
    let pointers = STACK + 0x300;
    let attributes = STACK + 0x400;
    let exe = STACK + 0x500;
    let startup_info = STACK + 0x600;
    let process_info = STACK + 0x700;
    mem.write_int(size, 8, 0).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "InitializeProcThreadAttributeList",
            &[0, 1, 0, size]
        ),
        0
    );
    assert_eq!(mem.read_int(size, 8).unwrap(), 64);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "InitializeProcThreadAttributeList",
            &[list, 1, 0, size]
        ),
        1
    );
    mem.write(attributes, &[0; 24]).unwrap();
    mem.write_int(attributes, 4, 24).unwrap();
    mem.write_int(attributes + 16, 4, 1).unwrap();
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreatePipe",
        &[pointers, pointers + 8, attributes, 0],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "UpdateProcThreadAttribute",
            &[list, 0, 0x20002, pointers, 8, 0, 0]
        ),
        1
    );
    let root = std::env::temp_dir().join(format!("wire64-missing-child-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    mem.write(startup_info, &[0; 104]).unwrap();
    mem.write_int(startup_info, 4, 104).unwrap();
    mem.write(process_info, &[0x55; 24]).unwrap();
    mem.write(
        exe,
        &"C:\\missing.exe\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateProcessW",
            &[exe, 0, 0, 0, 1, 0, 0, 0, startup_info, process_info]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        2
    );
    let mut unchanged = [0; 24];
    mem.read(process_info, &mut unchanged).unwrap();
    assert_eq!(unchanged, [0x55; 24]);
    mem.write(
        exe,
        &"\"C:\\missing.exe\" --attach\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateProcessW",
            &[0, exe, 0, 0, 1, 0, 0, 0, startup_info, process_info]
        ),
        0
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "DeleteProcThreadAttributeList",
        &[list],
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn win64_shell_argv_uses_high_pointers_shell_quoting_and_one_local_allocation() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let input = STACK + 0x100;
    let count = STACK + 0x500;
    for (command, expected) in [
        (
            "\"C:\\日本語 game.exe\" \"a b\" empty\"\" \\\"x",
            vec!["C:\\日本語 game.exe", "a b", "empty", "\"x"],
        ),
        ("  game.exe arg", vec!["", "game.exe", "arg"]),
        ("game.exe \"\" tail\\", vec!["game.exe", "", "tail\\"]),
    ] {
        mem.write(
            input,
            &command
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cpu.regs[1] = input;
        cpu.regs[2] = count;
        mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
        runtime
            .dispatch("shell32.dll", "CommandLineToArgvW", &mut cpu, &mut mem)
            .unwrap();
        let argv = cpu.regs[0];
        assert!(argv > u32::MAX as u64);
        assert_eq!(mem.read_int(count, 4).unwrap(), expected.len() as u64);
        for (n, expected) in expected.iter().enumerate() {
            let pointer = mem.read_int(argv + n as u64 * 8, 8).unwrap();
            let units: Vec<u16> = expected.encode_utf16().chain(Some(0)).collect();
            for (i, unit) in units.iter().enumerate() {
                assert_eq!(
                    mem.read_int(pointer + i as u64 * 2, 2).unwrap(),
                    *unit as u64
                );
            }
        }
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "LocalFree", &[argv]),
            0
        );
        assert!(mem.read_int(argv, 8).is_err());
    }
}

#[test]
fn win64_memory_status_reports_actual_guest_commit_limit_and_reservations() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let pointer = STACK + 0x100;
    mem.write_int(pointer, 4, 64).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GlobalMemoryStatusEx",
            &[pointer]
        ),
        1
    );
    let before = mem.read_int(pointer + 16, 8).unwrap();
    assert_eq!(
        mem.read_int(pointer + 8, 8).unwrap(),
        2 * 1024 * 1024 * 1024
    );
    let allocation = mem.virtual_alloc(0, 8192, 0x3000, 4).unwrap();
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GlobalMemoryStatusEx",
        &[pointer],
    );
    assert_eq!(mem.read_int(pointer + 16, 8).unwrap(), before - 8192);
    mem.virtual_free(allocation, 0, 0x8000).unwrap();
    mem.write_int(pointer, 4, 63).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GlobalMemoryStatusEx",
            &[pointer]
        ),
        0
    );
}

#[test]
fn win64_environment_is_guest_owned_case_insensitive_and_length_queries_do_not_write() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let name = STACK + 0x100;
    let value = STACK + 0x200;
    let output = STACK + 0x300;
    mem.write(name, b"Wire_Test\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetEnvironmentVariableA",
            &[name, 0, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        203
    );
    mem.write(value, b"guest-value\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetEnvironmentVariableA",
            &[name, value]
        ),
        1
    );
    mem.write(name, b"wire_test\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetEnvironmentVariableA",
            &[name, 0, 0]
        ),
        12
    );
    mem.write(output, &[0x55; 12]).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetEnvironmentVariableA",
            &[name, output, 11]
        ),
        12
    );
    assert_eq!(mem.read_int(output, 1).unwrap(), 0x55);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetEnvironmentVariableA",
            &[name, output, 12]
        ),
        11
    );
    let block = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetEnvironmentStringsW",
        &[],
    );
    assert_eq!(mem.read_int(block, 2).unwrap(), b'W' as u64);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "FreeEnvironmentStringsW",
            &[block]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetEnvironmentVariableA",
            &[name, 0]
        ),
        1
    );
}

#[test]
fn win64_bcrypt_uses_os_entropy_checks_output_and_tracks_rng_provider_lifetime() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let name = STACK + 0x100;
    let handle = STACK + 0x200;
    let output = STACK + 0x300;
    mem.write(
        name,
        &"RNG\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let call = |function: &str,
                args: &[u64],
                mem: &mut Memory,
                cpu: &mut Cpu64,
                runtime: &mut wire::x64::runtime::Runtime64| {
        for (n, value) in args.iter().enumerate() {
            cpu.regs[[1, 2, 8, 9][n]] = *value;
        }
        mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
        runtime.dispatch("bcrypt.dll", function, cpu, mem).unwrap();
        cpu.regs[0]
    };
    assert_eq!(
        call(
            "BCryptOpenAlgorithmProvider",
            &[handle, name, 0, 0],
            &mut mem,
            &mut cpu,
            &mut runtime
        ),
        0
    );
    let rng = mem.read_int(handle, 8).unwrap();
    mem.write(output - 1, &[0x55; 514]).unwrap();
    assert_eq!(
        call(
            "BCryptGenRandom",
            &[rng, output, 512, 0],
            &mut mem,
            &mut cpu,
            &mut runtime
        ),
        0
    );
    assert_eq!(mem.read_int(output - 1, 1).unwrap(), 0x55);
    assert_eq!(mem.read_int(output + 512, 1).unwrap(), 0x55);
    assert_eq!(
        call(
            "BCryptCloseAlgorithmProvider",
            &[rng, 0],
            &mut mem,
            &mut cpu,
            &mut runtime
        ),
        0
    );
    assert_eq!(
        call(
            "BCryptGenRandom",
            &[rng, output, 1, 0],
            &mut mem,
            &mut cpu,
            &mut runtime
        ),
        0xc0000008
    );
    assert_eq!(
        call(
            "BCryptGenRandom",
            &[0, output, 256, 2],
            &mut mem,
            &mut cpu,
            &mut runtime
        ),
        0
    );
    cpu.regs[1] = 0;
    cpu.regs[2] = STACK + 8192 - 16;
    cpu.regs[8] = 32;
    cpu.regs[9] = 2;
    mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
    let before = cpu.clone();
    assert!(runtime
        .dispatch("bcrypt.dll", "BCryptGenRandom", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
}

#[test]
fn win64_processor_topology_is_consistent_with_single_guest_cpu_and_x64_record_layouts() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x100;
    let size = STACK + 0x300;
    mem.write_int(size, 4, 0).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetLogicalProcessorInformationEx",
            &[0xffff, 0, size]
        ),
        0
    );
    assert_eq!(mem.read_int(size, 4).unwrap(), 224);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetLogicalProcessorInformationEx",
            &[0xffff, output, size]
        ),
        1
    );
    let mut at = output;
    for (kind, length) in [(0, 48), (1, 48), (3, 48), (4, 80)] {
        assert_eq!(mem.read_int(at, 4).unwrap(), kind);
        assert_eq!(mem.read_int(at + 4, 4).unwrap(), length);
        assert_eq!(
            mem.read_int(at + if kind == 4 { 72 } else { 32 }, 8)
                .unwrap(),
            1
        );
        at += length;
    }
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetActiveProcessorCount",
            &[0xffff]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetActiveProcessorCount",
            &[1]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetLogicalProcessorInformationEx",
            &[2, output, size]
        ),
        1
    );
    assert_eq!(mem.read_int(size, 4).unwrap(), 0);
}

#[test]
fn win64_codepage_stack_arguments_and_explicit_stdio_profile() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let text = "日本語\0";
    let encoded = wire::codepage::encode(text, 932).unwrap().0;
    let source = STACK + 0x100;
    let dest = STACK + 0x200;
    mem.write(source, &encoded).unwrap();
    let count = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "MultiByteToWideChar",
        &[932, 0, source, u32::MAX as u64, 0, 0xff7fff7e00000000],
    );
    assert_eq!(count, 4);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "MultiByteToWideChar",
            &[932, 0, source, u32::MAX as u64, dest, count]
        ),
        count
    );
    let mut bytes = [0; 8];
    mem.read(dest, &mut bytes).unwrap();
    assert_eq!(
        bytes.to_vec(),
        text.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    let converted = STACK + 0x300;
    let used = STACK + 0x400;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WideCharToMultiByte",
            &[932, 0, dest, count, converted, 32, 0, used]
        ),
        encoded.len() as u64
    );
    assert_eq!(mem.read_int(used, 4).unwrap(), 0);
    let mut output = vec![0; encoded.len()];
    mem.read(converted, &mut output).unwrap();
    assert_eq!(output, encoded);
    let handle = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetStdHandle",
        &[0xfffffff5],
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetFileType", &[handle]),
        3
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetConsoleMode",
            &[handle, used]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        6
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[handle]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetFileType", &[handle]),
        0
    );
}

#[test]
fn win64_toolhelp_snapshots_have_correct_x64_layout_and_checked_cursor_progress() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let out = STACK + 0x100;
    let snap = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateToolhelp32Snapshot",
        &[2, 0],
    );
    assert_ne!(snap, u64::MAX);
    mem.write_int(out, 4, 568).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "Process32FirstW",
            &[snap, out]
        ),
        1
    );
    assert_eq!(mem.read_int(out + 8, 4).unwrap(), 1);
    assert_eq!(mem.read_int(out + 16, 8).unwrap(), 0);
    assert_eq!(mem.read_int(out + 28, 4).unwrap(), 1);
    assert_eq!(mem.read_int(out + 36, 4).unwrap(), 8);
    let mut bytes = [0; 20];
    mem.read(out + 44, &mut bytes).unwrap();
    assert_eq!(
        bytes.to_vec(),
        "probe.exe\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "Process32NextW",
            &[snap, out]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        18
    );
    // A failed First copy cannot advance or reset a snapshot's iteration cursor.
    let bad = STACK + 8192 - 4;
    mem.write_int(bad, 4, 568).unwrap();
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = snap;
    cpu.regs[2] = bad;
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "Process32FirstW", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "Process32NextW",
            &[snap, out]
        ),
        0
    );
    mem.write_int(out, 4, 28).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "Thread32First",
            &[snap, out]
        ),
        0
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetHandleInformation",
        &[snap, 2, 2],
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[snap]),
        0
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetHandleInformation",
        &[snap, 2, 0],
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[snap]),
        1
    );
    let snap = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateToolhelp32Snapshot",
        &[4, 0],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "Thread32First",
            &[snap, out]
        ),
        1
    );
    assert_eq!(mem.read_int(out + 8, 4).unwrap(), 1);
    assert_eq!(mem.read_int(out + 12, 4).unwrap(), 1);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "Thread32Next",
            &[snap, out]
        ),
        0
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[snap]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateToolhelp32Snapshot",
            &[8, 0]
        ),
        u64::MAX
    );
}

#[test]
fn amd64_native_locale_callbacks_execute_stop_and_preserve_frames_across_budgets() {
    use wire::x64::process::{Outcome, Process64};
    let path = std::env::temp_dir().join(format!("wire-callback64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for keep_going in [false, true] {
        let mut p = Process64::load(&path, false).unwrap();
        let base = 0x500000000u64;
        p.memory.map(base, 8192, READ | WRITE | EXECUTE).unwrap();
        let gateway = p.runtime.register("kernel32.dll", "EnumSystemLocalesEx");
        let parameter = 0xaabbccdd12345678u64;
        let mut code = vec![0x48, 0x83, 0xec, 0x28, 0x48, 0xb9];
        code.extend(base.to_le_bytes());
        code.extend([0x31, 0xd2, 0x49, 0xb8]);
        code.extend(parameter.to_le_bytes());
        code.extend([0x45, 0x31, 0xc9, 0x48, 0xb8]);
        code.extend(gateway.to_le_bytes());
        code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(BASE + 0x1000, &code).unwrap();
        let mut callback = vec![0x48, 0xb8];
        callback.extend((base + 4096).to_le_bytes());
        callback.extend([
            0x48,
            0xff,
            0x00,
            0x48,
            0x89,
            0x48,
            8,
            0x4c,
            0x89,
            0x40,
            16,
            0xb8,
            keep_going as u8,
            0,
            0,
            0,
            0xc3,
        ]);
        p.memory.write(base, &callback).unwrap();
        let mut finished = false;
        for budget in 1..100 {
            if p.run(budget).unwrap() == Outcome::Returned(1) {
                finished = true;
                break;
            }
        }
        assert!(finished);
        assert_eq!(
            p.memory.read_int(base + 4096, 8).unwrap(),
            if keep_going { 4 } else { 1 }
        );
        assert_eq!(p.memory.read_int(base + 4112, 8).unwrap(), parameter);
        let temporary = p.memory.read_int(base + 4104, 8).unwrap();
        assert!(p.memory.read_int(temporary, 1).is_err());
        assert_eq!(p.cpu.regs[4], 0x7ff0000fff70); // Original EXE return frame, no callback stack leak.
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn win64_locale_queries_and_calendar_formats_preflight_outputs_and_keep_literals() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let locale = STACK + 0x100;
    let pattern = STACK + 0x200;
    let time = STACK + 0x300;
    let out = STACK + 0x400;
    fn wide(mem: &mut Memory, ptr: u64, text: &str) {
        mem.write(
            ptr,
            &text
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    wide(&mut mem, locale, "en-US");
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "LocaleNameToLCID",
            &[locale, 0]
        ),
        0x409
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetLocaleInfoEx",
            &[locale, 0x20001004, out, 2]
        ),
        2
    );
    assert_eq!(mem.read_int(out, 4).unwrap(), 1252);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetLocaleInfoEx",
            &[0, 0x20001004, out, 2]
        ),
        2
    );
    assert_eq!(mem.read_int(out, 4).unwrap(), 932);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetLocaleInfoEx",
            &[locale, 0x1004, 0, 0]
        ),
        5
    );
    wide(&mut mem, pattern, "yyyy-MM-dd 'year' '' dddd");
    for (n, v) in [2024u16, 2, 0, 29, 99, 99, 99, 99].iter().enumerate() {
        mem.write_int(time + n as u64 * 2, 2, *v as u64).unwrap();
    }
    let count = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetDateFormatEx",
        &[locale, 0, time, pattern, out, 128, 0],
    );
    let mut bytes = vec![0; count as usize * 2];
    mem.read(out, &mut bytes).unwrap();
    assert_eq!(
        bytes,
        "2024-02-29 year ' Thursday\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    // Date ignores input time and the supplied weekday; Time ignores the date.
    for (n, v) in [0u16, 0, 0, 0, 13, 4, 5, 999].iter().enumerate() {
        mem.write_int(time + n as u64 * 2, 2, *v as u64).unwrap();
    }
    wide(&mut mem, pattern, "HH:mm:ss 'M yyyy' tt");
    let count = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetTimeFormatEx",
        &[locale, 0, time, pattern, out, 128],
    );
    let mut bytes = vec![0; count as usize * 2];
    mem.read(out, &mut bytes).unwrap();
    assert_eq!(
        bytes,
        "13:04:05 M yyyy PM\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    mem.write_int(out, 8, 0xfeed).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetTimeFormatEx",
            &[locale, 0, time, pattern, out, 2]
        ),
        0
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xfeed);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        122
    );
    wide(&mut mem, locale, "unknown-XX");
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "IsValidLocaleName",
            &[locale]
        ),
        0
    );
    wide(&mut mem, locale, "FunctionWireDoesNotProvide");
    let kernel = startup_call(&mut mem, &mut cpu, &mut runtime, "GetModuleHandleW", &[0]);
    assert_ne!(kernel, 0); // Main image is a real guest module, not a provider handle.
    mem.write(pattern, b"kernel32.dll\0").unwrap();
    runtime.install_gateways(Default::default());
    let kernel = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetModuleHandleA",
        &[pattern],
    );
    mem.write(pattern, b"FunctionWireDoesNotProvide\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[kernel, pattern]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        127
    );
}

#[test]
fn win64_known_folders_use_confined_real_storage_and_com_allocation_lifetime() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let root = std::env::temp_dir().join(format!("wire-known64-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    runtime.set_filesystem(wire::fs::GuestFs::new(&root).unwrap());
    let guid = STACK + 0x100;
    let out = STACK + 0x200;
    let path = STACK + 0x300;
    mem.write(
        guid,
        &[
            0xa4, 0xa1, 0x20, 0xa5, 0x80, 0x17, 0xf6, 0x4f, 0xbd, 0x18, 0x16, 0x73, 0x43, 0xc5,
            0xaf, 0x16,
        ],
    )
    .unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SHGetKnownFolderPath",
            &[guid, 0x8000, 0, out]
        ),
        0
    );
    let pointer = mem.read_int(out, 8).unwrap();
    assert!(root.join("Users/wire/AppData/LocalLow").is_dir());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileAttributesW",
            &[pointer]
        ),
        0x10
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetFileAttributesW",
            &[pointer, 0x2010]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetFileAttributesW",
            &[pointer]
        ),
        0x2010
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CoTaskMemFree",
        &[pointer],
    );
    assert!(mem.read_int(pointer, 1).is_err());
    mem.write(guid, &[0; 16]).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SHGetKnownFolderPath",
            &[guid, 0, 0, out]
        ),
        0x80070032
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0);
    mem.write(path, b"C:\\..\\escape\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateDirectoryA",
            &[path, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        5
    );
    std::fs::remove_dir_all(root).unwrap();
}
