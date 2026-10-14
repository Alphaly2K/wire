//! Existing behavioral tests grouped by com; expectations remain independent of API catalogs.

use super::*;

#[test]
fn win64_bstr_ordinals_byte_lengths_alias_reallocation_and_faults_preserve_owned_data() {
    let (mut m, mut cpu, mut r) = startup();
    let source = STACK + 0x100;
    let output = STACK + 0x180;
    m.write(source, &[0x41, 0, 0, 0, 0, 0xd8, 0, 0]).unwrap();
    let p = startup_dll_call(
        &mut m,
        &mut cpu,
        &mut r,
        "oleaut32.dll",
        "#4",
        &[source, 0xabcdefff00000003],
    );
    assert!(p > u32::MAX as u64);
    assert_eq!(m.read_int(p - 4, 4).unwrap(), 6);
    assert_eq!(
        startup_dll_call(&mut m, &mut cpu, &mut r, "oleaut32.dll", "#7", &[p]),
        3
    );
    assert_eq!(
        startup_dll_call(&mut m, &mut cpu, &mut r, "oleaut32.dll", "#149", &[p]),
        6
    );
    let mut content = [0; 8];
    m.read(p, &mut content).unwrap();
    assert_eq!(content, [0x41, 0, 0, 0, 0, 0xd8, 0, 0]);
    m.write_int(output, 8, p).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "oleaut32.dll",
            "#5",
            &[output, p + 2, 2]
        ),
        1
    );
    let next = m.read_int(output, 8).unwrap();
    assert!(m.read_int(p, 2).is_err());
    let mut content = [0; 6];
    m.read(next, &mut content).unwrap();
    assert_eq!(content, [0, 0, 0, 0xd8, 0, 0]);
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "oleaut32.dll",
            "SysReAllocStringLen",
            &[output, 0, 4]
        ),
        1
    );
    let expanded = m.read_int(output, 8).unwrap();
    let mut content = [0; 10];
    m.read(expanded, &mut content).unwrap();
    assert_eq!(content, [0, 0, 0, 0xd8, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "oleaut32.dll",
            "SysReAllocStringLen",
            &[output, source, u32::MAX as u64]
        ),
        0
    );
    assert_eq!(m.read_int(output, 8).unwrap(), expanded);
    let before = cpu.clone();
    let api = r.api_calls;
    cpu.regs[1] = output;
    cpu.regs[2] = STACK + 8190;
    cpu.regs[8] = 3;
    let fault_cpu = cpu.clone();
    assert!(r
        .dispatch("oleaut32.dll", "SysReAllocStringLen", &mut cpu, &mut m)
        .is_err());
    assert_eq!(cpu.regs, fault_cpu.regs);
    assert_eq!(cpu.rip, fault_cpu.rip);
    assert_eq!(r.api_calls, api);
    assert_eq!(m.read_int(output, 8).unwrap(), expanded);
    assert_eq!(m.read_int(expanded - 4, 4).unwrap(), 8);
    cpu = before;
    m.write(source, b"abc").unwrap();
    let bytes = startup_dll_call(
        &mut m,
        &mut cpu,
        &mut r,
        "oleaut32.dll",
        "#150",
        &[source, 3],
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "oleaut32.dll",
            "SysStringLen",
            &[bytes]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "oleaut32.dll",
            "SysStringByteLen",
            &[bytes]
        ),
        3
    );
    let mut content = [0; 5];
    m.read(bytes, &mut content).unwrap();
    assert_eq!(content, *b"abc\0\0");
    assert_eq!(
        startup_dll_call(&mut m, &mut cpu, &mut r, "oleaut32.dll", "#3", &[output, 0]),
        1
    );
    assert_eq!(m.read_int(output, 8).unwrap(), 0);
    assert!(m.read_int(expanded, 2).is_err());
    startup_dll_call(&mut m, &mut cpu, &mut r, "oleaut32.dll", "#6", &[bytes]);
    assert!(m.read_int(bytes, 1).is_err());
    assert_eq!(
        startup_dll_call(&mut m, &mut cpu, &mut r, "oleaut32.dll", "#2", &[0]),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "oleaut32.dll",
            "SysFreeString",
            &[0]
        ),
        0
    );
    m.write(source, b"oleaut32.dll\0").unwrap();
    let module = startup_call(&mut m, &mut cpu, &mut r, "LoadLibraryA", &[source]);
    let gateway = startup_call(&mut m, &mut cpu, &mut r, "GetProcAddress", &[module, 2]);
    assert_eq!(r.gateways[&gateway], "oleaut32.dll!SysAllocString");
    assert!(!wire::x64::runtime::import_supported(
        "kernel32.dll",
        "SysAllocString"
    ));
    assert!(wire::x64::runtime::import_supported("oleaut32.dll", "#150"));
}

#[test]
fn win64_com_missing_factory_and_multimedia_timer_reference_balance() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let data = STACK + 0x200;
    mem.write(data, &[0x55; 40]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "CoCreateInstance",
            &[data, 0, 1, data + 16, data + 32]
        ),
        0x800401f0
    );
    assert_eq!(mem.read_int(data + 32, 8).unwrap(), 0);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "CoInitializeEx",
            &[0, 0]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "CoCreateInstance",
            &[data, 0, 1, data + 16, data + 32]
        ),
        0x80040154
    );
    for (name, period, expected) in [
        ("timeEndPeriod", 1, 97),
        ("timeBeginPeriod", 0, 97),
        ("timeBeginPeriod", 1, 0),
        ("timeBeginPeriod", 1, 0),
        ("timeEndPeriod", 1, 0),
        ("timeEndPeriod", 1, 0),
        ("timeEndPeriod", 1, 97),
    ] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "winmm.dll",
                name,
                &[period]
            ),
            expected
        );
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "winmm.dll",
            "timeGetDevCaps",
            &[data, 8]
        ),
        0
    );
    assert_eq!(mem.read_int(data, 4).unwrap(), 1);
    assert_eq!(mem.read_int(data + 4, 4).unwrap(), u32::MAX as u64);
}

#[test]
fn win64_guid_entropy_layout_string_roundtrip_and_task_ownership() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let guid = STACK + 0x100;
    let string = STACK + 0x200;
    let parsed = STACK + 0x300;
    let output = STACK + 0x400;
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetLastError", &[0x1234]);
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..32 {
        mem.write(guid - 1, &[0x55; 18]).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "ole32.dll",
                "CoCreateGuid",
                &[guid]
            ),
            0
        );
        let mut bytes = [0; 16];
        mem.read(guid, &mut bytes).unwrap();
        assert_eq!(bytes[7] >> 4, 4);
        assert_eq!(bytes[8] >> 6, 2);
        assert!(seen.insert(bytes));
        assert_eq!(mem.read_int(guid - 1, 1).unwrap(), 0x55);
        assert_eq!(mem.read_int(guid + 16, 1).unwrap(), 0x55);
    }
    let bytes = [
        0x67, 0x45, 0x23, 0x01, 0xab, 0x89, 0xef, 0xcd, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd,
        0xef,
    ];
    mem.write(guid, &bytes).unwrap();
    let expected = "{01234567-89AB-CDEF-0123-456789ABCDEF}\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    mem.write(string, &[0x55; 80]).unwrap();
    for capacity in [0, 38, u32::MAX as u64] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "ole32.dll",
                "StringFromGUID2",
                &[guid, string, capacity]
            ),
            0
        );
        assert_eq!(mem.read_int(string, 8).unwrap(), 0x5555555555555555);
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "combase.dll",
            "StringFromGUID2",
            &[guid, string, 0x100000027]
        ),
        39
    );
    let mut actual = [0; 78];
    mem.read(string, &mut actual).unwrap();
    assert_eq!(actual.as_slice(), expected);
    assert_eq!(mem.read_int(string + 78, 2).unwrap(), 0x5555);
    for name in ["CLSIDFromString", "IIDFromString"] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "ole32.dll",
                name,
                &[string, parsed]
            ),
            0
        );
        let mut actual = [0; 16];
        mem.read(parsed, &mut actual).unwrap();
        assert_eq!(actual, bytes);
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "combase.dll",
                name,
                &[0, parsed]
            ),
            0
        );
        mem.read(parsed, &mut actual).unwrap();
        assert_eq!(actual, [0; 16]);
    }
    for name in ["StringFromCLSID", "StringFromIID"] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "ole32.dll",
                name,
                &[guid, output]
            ),
            0
        );
        let pointer = mem.read_int(output, 8).unwrap();
        assert!(pointer > u32::MAX as u64);
        mem.read(pointer, &mut actual).unwrap();
        assert_eq!(actual.as_slice(), expected);
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "CoTaskMemFree",
            &[pointer],
        );
        assert!(mem.read_int(pointer, 1).is_err());
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "CoCreateGuid",
            &[0]
        ),
        0x80070057
    );
    mem.write(
        string,
        &"bad\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "IIDFromString",
            &[string, parsed]
        ),
        0x80070057
    );
    mem.write(string, &expected).unwrap();
    mem.write_int(string + 2, 2, b'Z' as u64).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "IIDFromString",
            &[string, parsed]
        ),
        0x800401f4
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ole32.dll",
            "CLSIDFromString",
            &[string, parsed]
        ),
        0x800401f3
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0x1234
    );
}

#[test]
fn win64_hstrings_preserve_reference_lifetimes_owned_copies_and_utf16_units() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let source = STACK + 0x100;
    let header = STACK + 0x200;
    let output = STACK + 0x300;
    let length = STACK + 0x400;
    let iid = STACK + 0x500;
    macro_rules! call {
        ($name:expr, $args:expr) => {
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "combase.dll",
                $name,
                $args,
            )
        };
    }
    let data = [0x0041u16, 0, 0xd800, 0x65e5, 0];
    mem.write(
        source,
        &data
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    mem.write(header, &[0x55; 24]).unwrap();
    assert_eq!(
        call!(
            "WindowsCreateStringReference",
            &[source, 0x100000004, header, output]
        ),
        0
    );
    let reference = mem.read_int(output, 8).unwrap();
    assert_eq!(reference, header);
    assert_eq!(mem.read_int(header + 8, 8).unwrap(), 0x5555555555555555);
    assert_eq!(call!("WindowsGetStringLen", &[reference]), 4);
    assert_eq!(
        call!("WindowsGetStringRawBuffer", &[reference, length]),
        source
    );
    assert_eq!(mem.read_int(length, 4).unwrap(), 4);
    assert_eq!(
        call!("WindowsStringHasEmbeddedNull", &[reference, length]),
        0
    );
    assert_eq!(mem.read_int(length, 4).unwrap(), 1);
    assert_eq!(call!("WindowsDuplicateString", &[reference, output]), 0);
    let owned = mem.read_int(output, 8).unwrap();
    assert!(owned > u32::MAX as u64 && owned != reference);
    assert_eq!(
        call!("WindowsCompareStringOrdinal", &[reference, owned, length]),
        0
    );
    assert_eq!(mem.read_int(length, 4).unwrap(), 0);
    mem.write_int(source, 2, 0x42).unwrap();
    assert_eq!(call!("WindowsGetStringRawBuffer", &[reference, 0]), source);
    let buffer = call!("WindowsGetStringRawBuffer", &[owned, 0]);
    assert_eq!(mem.read_int(buffer, 2).unwrap(), 0x41);
    assert!(mem.write(buffer, &[0; 2]).is_err());
    assert_eq!(
        call!("WindowsCompareStringOrdinal", &[reference, owned, length]),
        0
    );
    assert_eq!(mem.read_int(length, 4).unwrap(), 1);
    assert_eq!(call!("WindowsDeleteString", &[reference]), 0);
    assert_eq!(call!("WindowsGetStringLen", &[reference]), 4);
    assert_eq!(call!("WindowsDuplicateString", &[owned, output]), 0);
    assert_eq!(mem.read_int(output, 8).unwrap(), owned);
    assert_eq!(call!("WindowsDeleteString", &[owned]), 0);
    assert_eq!(call!("WindowsGetStringLen", &[owned]), 4);
    assert_eq!(call!("WindowsDeleteString", &[owned]), 0);
    assert!(mem.read_int(buffer, 2).is_err());
    assert_eq!(call!("WindowsCreateString", &[source, 4, output]), 0);
    let created = mem.read_int(output, 8).unwrap();
    assert_eq!(call!("WindowsGetStringLen", &[created]), 4);
    assert_eq!(call!("WindowsDeleteString", &[created]), 0);
    assert_eq!(call!("WindowsCreateString", &[0, 0, output]), 0);
    assert_eq!(mem.read_int(output, 8).unwrap(), 0);
    assert_eq!(call!("WindowsIsStringEmpty", &[0]), 1);
    let empty = call!("WindowsGetStringRawBuffer", &[0, length]);
    assert!(empty > u32::MAX as u64);
    assert_eq!(mem.read_int(empty, 2).unwrap(), 0);
    assert_eq!(mem.read_int(length, 4).unwrap(), 0);
    assert!(mem.write(empty, &[1]).is_err());
    mem.write_int(output, 8, 0x1234567812345678).unwrap();
    assert_eq!(
        call!("RoGetActivationFactory", &[reference, iid, output]),
        0x80040154
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 0x1234567812345678);
    assert_eq!(
        call!("RoActivateInstance", &[reference, output]),
        0x80040154
    );
    assert_eq!(call!("WindowsDeleteString", &[0]), 0);
}

#[test]
fn win64_hstring_outputs_fault_before_headers_allocations_or_reference_counts_change() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let source = STACK + 0x100;
    let header = STACK + 0x200;
    let out = STACK + 0x300;
    mem.write(source, &[0x41, 0, 0, 0]).unwrap();
    mem.write(header, &[0x55; 24]).unwrap();
    mem.write_int(out, 8, 0x5555555555555555).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "combase.dll",
            "WindowsCreateStringReference",
            &[source, 0, header, out]
        ),
        0x80070057
    );
    assert_eq!(mem.read_int(header, 8).unwrap(), 0x5555555555555555);
    assert_eq!(mem.read_int(out, 8).unwrap(), 0x5555555555555555);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "combase.dll",
            "WindowsCreateString",
            &[source, 1, out]
        ),
        0
    );
    let owned = mem.read_int(out, 8).unwrap();
    let memory = mem.memory_status();
    let bad = STACK + 4092;
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    for (name, args) in [
        ("WindowsCreateStringReference", [source, 1, header, bad]),
        ("WindowsCreateStringReference", [source, 1, bad, out]),
        ("WindowsCreateString", [source, 1, bad, 0]),
        ("WindowsDuplicateString", [owned, bad, 0, 0]),
    ] {
        for (n, value) in args.into_iter().enumerate() {
            cpu.regs[[1, 2, 8, 9][n]] = value;
        }
        let before = cpu.clone();
        let calls = runtime.api_calls;
        assert!(runtime
            .dispatch("combase.dll", name, &mut cpu, &mut mem)
            .is_err());
        assert_eq!(cpu.regs, before.regs);
        assert_eq!(cpu.rip, before.rip);
        assert_eq!(runtime.api_calls, calls);
        assert_eq!(mem.memory_status(), memory);
        assert_eq!(mem.read_int(header, 8).unwrap(), 0x5555555555555555);
        assert_eq!(mem.read_int(out, 8).unwrap(), owned);
    }
    mem.protect(STACK + 4096, 4096, READ | WRITE).unwrap();
    startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "combase.dll",
        "WindowsDeleteString",
        &[owned],
    );
    assert!(mem.read_int(owned, 1).is_err()); // failed duplicate retained no reference
    let module_name = STACK + 0x500;
    let symbol = STACK + 0x600;
    for dll in [
        "combase.dll",
        "api-ms-win-core-winrt-string-l1-1-0.dll",
        "kernel32.dll",
    ] {
        runtime.register_provider(dll);
        mem.write(module_name, format!("{dll}\0").as_bytes())
            .unwrap();
        let module = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetModuleHandleA",
            &[module_name],
        );
        mem.write(symbol, b"WindowsCreateStringReference\0")
            .unwrap();
        let address = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[module, symbol],
        );
        if dll == "kernel32.dll" {
            assert_eq!(address, 0);
        } else {
            assert_eq!(
                runtime.gateways[&address],
                "combase.dll!WindowsCreateStringReference"
            );
        }
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "user32.dll",
            "EnableMouseInPointer",
            &[1]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        50
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "user32.dll",
            "IsMouseInPointerEnabled",
            &[]
        ),
        0
    );
}

#[test]
fn win64_guid_faults_are_atomic_and_exports_are_dll_scoped() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let symbol = STACK + 0x100;
    let module_name = STACK + 0x200;
    for dll in ["ole32.dll", "combase.dll", "kernel32.dll"] {
        runtime.register_provider(dll);
        mem.write(module_name, format!("{dll}\0").as_bytes())
            .unwrap();
        let module = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetModuleHandleA",
            &[module_name],
        );
        assert_ne!(module, 0);
        for name in [
            "CoCreateGuid",
            "CLSIDFromString",
            "IIDFromString",
            "StringFromGUID2",
            "StringFromCLSID",
            "StringFromIID",
        ] {
            mem.write(symbol, format!("{name}\0").as_bytes()).unwrap();
            let address = startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetProcAddress",
                &[module, symbol],
            );
            if dll == "kernel32.dll" {
                assert_eq!(address, 0);
            } else {
                assert_eq!(runtime.gateways[&address], format!("{dll}!{name}"));
            }
        }
    }
    let output = STACK + 4090;
    mem.write(output, &[0x55; 16]).unwrap();
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    cpu.regs[1] = output;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("ole32.dll", "CoCreateGuid", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(runtime.api_calls, calls);
    let mut data = [0; 16];
    mem.read(output, &mut data).unwrap();
    assert_eq!(data, [0x55; 16]);
    // Checked failures also cover allocating string outputs without leaking an allocation.
    cpu.regs[1] = STACK + 0x100;
    cpu.regs[2] = output;
    let committed = mem.memory_status();
    assert!(runtime
        .dispatch("ole32.dll", "StringFromCLSID", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(mem.memory_status(), committed);
    assert_eq!(runtime.api_calls, calls);
}

#[test]
fn win64_winrt_initialization_balances_apartment_refs_and_rejects_mode_changes() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let call = |name: &str,
                arguments: &[u64],
                mem: &mut Memory,
                cpu: &mut Cpu64,
                runtime: &mut wire::x64::runtime::Runtime64| {
        for (n, value) in arguments.iter().enumerate() {
            cpu.regs[[1, 2, 8, 9][n]] = *value;
        }
        mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
        runtime
            .dispatch("api-ms-win-core-winrt-l1-1-0.dll", name, cpu, mem)
            .unwrap();
        cpu.regs[0]
    };
    assert_eq!(
        call("RoInitialize", &[0], &mut mem, &mut cpu, &mut runtime),
        0
    );
    assert_eq!(
        call("RoInitialize", &[0], &mut mem, &mut cpu, &mut runtime),
        1
    );
    assert_eq!(
        call("RoInitialize", &[1], &mut mem, &mut cpu, &mut runtime),
        0x80010106
    );
    call("RoUninitialize", &[], &mut mem, &mut cpu, &mut runtime);
    assert_eq!(
        call("CoInitializeEx", &[0, 2], &mut mem, &mut cpu, &mut runtime),
        1
    );
    call("RoUninitialize", &[], &mut mem, &mut cpu, &mut runtime);
    assert_eq!(
        call("CoInitializeEx", &[0, 0], &mut mem, &mut cpu, &mut runtime),
        0x80010106
    );
    call("CoUninitialize", &[], &mut mem, &mut cpu, &mut runtime);
    assert_eq!(
        call("RoInitialize", &[1], &mut mem, &mut cpu, &mut runtime),
        0
    );
    assert_eq!(
        call("RoInitialize", &[2], &mut mem, &mut cpu, &mut runtime),
        0x80070057
    );
}
