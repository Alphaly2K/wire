//! Existing behavioral tests grouped by network; expectations remain independent of API catalogs.

use super::*;

#[test]
fn win64_absent_socket_transport_returns_pointer_width_failure_without_creating_handles() {
    let (mut m, mut cpu, mut r) = startup();
    let out = STACK + 0x100;
    for name in ["socket", "#23", "WSASocketA", "WSASocketW"] {
        let args = if name.starts_with("WSA") {
            vec![2, 1, 6, 0, 0, 1]
        } else {
            vec![2, 1, 6]
        };
        assert_eq!(
            startup_dll_call(&mut m, &mut cpu, &mut r, "ws2_32.dll", name, &args),
            u64::MAX
        );
        assert_eq!(
            startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
            10093
        );
    }
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSAStartup",
            &[0x202, out]
        ),
        0
    );
    for name in ["socket", "#23", "WSASocketA", "WSASocketW"] {
        let args = if name.starts_with("WSA") {
            vec![2, 1, 6, 0, 0, 1]
        } else {
            vec![2, 1, 6]
        };
        assert_eq!(
            startup_dll_call(&mut m, &mut cpu, &mut r, "ws2_32.dll", name, &args),
            u64::MAX
        );
        assert_eq!(
            startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
            10047
        );
        assert!(!wire::x64::runtime::import_supported("kernel32.dll", name));
    }
    m.write(out, &[0xaa; 16]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSASocketW",
            &[u64::MAX, u64::MAX, u64::MAX, out, 0, 0]
        ),
        u64::MAX
    );
    assert_eq!(
        startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
        10045
    );
    assert_eq!(m.read_int(out, 8).unwrap(), 0xaaaaaaaaaaaaaaaa);
    let event = startup_dll_call(
        &mut m,
        &mut cpu,
        &mut r,
        "ws2_32.dll",
        "WSACreateEvent",
        &[],
    );
    assert_ne!(event, 0);
    assert_ne!(event, u64::MAX);
    assert_eq!(
        startup_call(&mut m, &mut cpu, &mut r, "WaitForSingleObject", &[event, 0]),
        258
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSACloseEvent",
            &[event]
        ),
        1
    );
}

#[test]
fn win64_ip_addresses_use_windows_families_network_bytes_and_checked_full_width_outputs() {
    let (mut m, mut cpu, mut r) = startup();
    let source = STACK + 0x100;
    let address = STACK + 0x200;
    let output = STACK + 0x300;
    for (text, family, expected) in [
        ("127.2.3.255", 2, vec![127, 2, 3, 255]),
        (
            "::1",
            23,
            vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        ),
        (
            "2001:db8::12",
            23,
            vec![0x20, 1, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x12],
        ),
        (
            "::ffff:192.0.2.10",
            23,
            vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 192, 0, 2, 10],
        ),
        (
            "::192.0.2.10",
            23,
            vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 192, 0, 2, 10],
        ),
    ] {
        m.write(source, format!("{text}\0").as_bytes()).unwrap();
        startup_call(&mut m, &mut cpu, &mut r, "SetLastError", &[0xdeadbeef]);
        assert_eq!(
            startup_dll_call(
                &mut m,
                &mut cpu,
                &mut r,
                "ws2_32.dll",
                "inet_pton",
                &[0xabcd00000000 | family, source, address]
            ),
            1
        );
        let mut bytes = vec![0; expected.len()];
        m.read(address, &mut bytes).unwrap();
        assert_eq!(bytes, expected);
        assert_eq!(
            startup_dll_call(
                &mut m,
                &mut cpu,
                &mut r,
                "ws2_32.dll",
                "inet_ntop",
                &[family, address, output, 0x100000100]
            ),
            output
        );
        let mut rendered = vec![0; text.len() + 1];
        m.read(output, &mut rendered).unwrap();
        assert_eq!(rendered, format!("{text}\0").as_bytes());
        assert_eq!(
            startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
            0xdeadbeef
        );
        let wide: Vec<_> = text
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        m.write(source, &wide).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut m,
                &mut cpu,
                &mut r,
                "ws2_32.dll",
                "InetPtonW",
                &[family, source, address]
            ),
            1
        );
        assert_eq!(
            startup_dll_call(
                &mut m,
                &mut cpu,
                &mut r,
                "ws2_32.dll",
                "InetNtopW",
                &[family, address, output, text.len() as u64 + 1]
            ),
            output
        );
        let mut rendered = vec![0; wide.len()];
        m.read(output, &mut rendered).unwrap();
        assert_eq!(rendered, wide);
    }
    for (family, text) in [
        (2, "127.0.0.256"),
        (2, "127.1"),
        (2, "01.2.3.4"),
        (23, "fe80::1%2"),
        (23, "1::2::3"),
        (2, "\u{fffd}"),
    ] {
        m.write(address, &[0xaa; 16]).unwrap();
        m.write(source, format!("{text}\0").as_bytes()).unwrap();
        startup_call(&mut m, &mut cpu, &mut r, "SetLastError", &[42]);
        assert_eq!(
            startup_dll_call(
                &mut m,
                &mut cpu,
                &mut r,
                "ws2_32.dll",
                "inet_pton",
                &[family, source, address]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
            42
        );
        let mut bytes = [0; 16];
        m.read(address, &mut bytes).unwrap();
        assert_eq!(bytes, [0xaa; 16]);
    }
    m.write(source, &[0, 0xd8, 0, 0]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "InetPtonW",
            &[2, source, address]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
        10022
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "inet_pton",
            &[30, source, address]
        ),
        u32::MAX as u64
    );
    assert_eq!(
        startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
        10047
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "inet_pton",
            &[30, 0, address]
        ),
        u32::MAX as u64
    );
    assert_eq!(
        startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
        10014
    );
    m.write(address, &[127, 0, 0, 1]).unwrap();
    m.write(output, &[0xaa; 32]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "inet_ntop",
            &[2, address, output, 9]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
        0xc000000d
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "InetNtopW",
            &[2, address, output, 9]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
        87
    );
    let mut bytes = [0; 32];
    m.read(output, &mut bytes).unwrap();
    assert_eq!(bytes, [0xaa; 32]);
    // Overlapping source/destination is staged; a cross-page destination fault
    // commits neither partial bytes nor the API return/CPU state.
    m.write(source, b"127.0.0.1\0").unwrap();
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "inet_pton",
            &[2, source, source]
        ),
        1
    );
    assert_eq!(m.read_int(source, 4).unwrap(), 0x0100007f);
    m.write(source, b"2001:db8::12\0").unwrap();
    let tail = STACK + 8192 - 8;
    m.write(tail, &[0xaa; 8]).unwrap();
    cpu.regs[1] = 23;
    cpu.regs[2] = source;
    cpu.regs[8] = tail;
    let before = cpu.clone();
    let calls = r.api_calls;
    assert!(r
        .dispatch("ws2_32.dll", "inet_pton", &mut cpu, &mut m)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(r.api_calls, calls);
    assert_eq!(m.read_int(tail, 8).unwrap(), 0xaaaaaaaaaaaaaaaa);
    for (name, input, expected) in [
        ("#8", 0xabcdef0112345678, 0x78563412),
        ("#14", 0x78563412, 0x12345678),
        ("#9", 0xabcdef0112345678, 0x7856),
        ("#15", 0x7856, 0x5678),
    ] {
        assert_eq!(
            startup_dll_call(&mut m, &mut cpu, &mut r, "ws2_32.dll", name, &[input]),
            expected
        );
        assert!(!wire::x64::runtime::import_supported("kernel32.dll", name));
    }
    m.write(source, b"ws2_32.dll\0").unwrap();
    let module = startup_call(&mut m, &mut cpu, &mut r, "LoadLibraryA", &[source]);
    assert_ne!(module, 0);
    for (symbol, canonical) in [(source, "inet_pton"), (8, "htonl")] {
        m.write(source, b"inet_pton\0").unwrap();
        let gateway = startup_call(
            &mut m,
            &mut cpu,
            &mut r,
            "GetProcAddress",
            &[module, symbol],
        );
        assert_eq!(r.gateways[&gateway], format!("ws2_32.dll!{canonical}"));
        assert!(!wire::x64::runtime::import_supported(
            "kernelbase.dll",
            canonical
        ));
    }
}

#[test]
fn win64_winsock_events_are_owned_manual_reset_objects_and_waits_keep_pointer_width() {
    let (mut m, mut cpu, mut r) = startup();
    let out = STACK + 0x100;
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSACreateEvent",
            &[]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSAGetLastError",
            &[]
        ),
        10093
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSAStartup",
            &[0x202, out]
        ),
        0
    );
    let e = startup_dll_call(
        &mut m,
        &mut cpu,
        &mut r,
        "ws2_32.dll",
        "WSACreateEvent",
        &[],
    );
    assert_ne!(e, 0);
    m.write_int(out, 8, e).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSAWaitForMultipleEvents",
            &[1, out, 0, 0, 0]
        ),
        258
    );
    assert_eq!(
        startup_dll_call(&mut m, &mut cpu, &mut r, "ws2_32.dll", "WSASetEvent", &[e]),
        1
    );
    for _ in 0..2 {
        assert_eq!(
            startup_dll_call(
                &mut m,
                &mut cpu,
                &mut r,
                "ws2_32.dll",
                "WSAWaitForMultipleEvents",
                &[1, out, 1, 0, 0]
            ),
            0
        );
    }
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSAResetEvent",
            &[e]
        ),
        1
    );
    assert_eq!(
        startup_call(&mut m, &mut cpu, &mut r, "WaitForSingleObject", &[e, 0]),
        258
    );
    assert_eq!(startup_call(&mut m, &mut cpu, &mut r, "SetEvent", &[e]), 1);
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSAWaitForMultipleEvents",
            &[0xdeadbeef00000001, out, 0, 0, 0]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSAWaitForMultipleEvents",
            &[0, out, 0, 0, 0]
        ),
        u32::MAX as u64
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSAGetLastError",
            &[]
        ),
        10022
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSACloseEvent",
            &[e]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(&mut m, &mut cpu, &mut r, "ws2_32.dll", "WSASetEvent", &[e]),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "ws2_32.dll",
            "WSAGetLastError",
            &[]
        ),
        6
    );
    assert!(!wire::x64::runtime::import_supported(
        "kernel32.dll",
        "WSACreateEvent"
    ));
}

#[test]
fn win64_winhttp_proxy_profile_and_absent_transport_preserve_fault_outputs() {
    let (mut m, mut cpu, mut r) = startup();
    let out = STACK + 0x100;
    m.write(out, &[0xaa; 32]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "winhttp.dll",
            "WinHttpGetIEProxyConfigForCurrentUser",
            &[out]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
        2
    );
    let mut content = [0; 32];
    m.read(out, &mut content).unwrap();
    assert_eq!(content, [0xaa; 32]);
    cpu.regs[1] = STACK + 8176;
    let before = cpu.clone();
    let api = r.api_calls;
    assert!(r
        .dispatch(
            "winhttp.dll",
            "WinHttpGetIEProxyConfigForCurrentUser",
            &mut cpu,
            &mut m
        )
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(r.api_calls, api);
    assert_eq!(
        startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
        2
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "winhttp.dll",
            "WinHttpGetDefaultProxyConfiguration",
            &[out]
        ),
        1
    );
    m.read(out, &mut content).unwrap();
    assert_eq!(&content[..4], &1u32.to_le_bytes());
    assert_eq!(&content[4..24], &[0; 20]);
    assert_eq!(&content[24..], &[0xaa; 8]);
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "winhttp.dll",
            "WinHttpOpen",
            &[0, 1, 0, 0, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
        50
    );
    assert_eq!(
        startup_dll_call(
            &mut m,
            &mut cpu,
            &mut r,
            "winhttp.dll",
            "WinHttpCloseHandle",
            &[0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut m, &mut cpu, &mut r, "GetLastError", &[]),
        6
    );
    m.write(out, b"winhttp.dll\0").unwrap();
    let module = startup_call(&mut m, &mut cpu, &mut r, "LoadLibraryA", &[out]);
    m.write(out, b"WinHttpGetIEProxyConfigForCurrentUser\0")
        .unwrap();
    let gateway = startup_call(&mut m, &mut cpu, &mut r, "GetProcAddress", &[module, out]);
    assert_eq!(
        r.gateways[&gateway],
        "winhttp.dll!WinHttpGetIEProxyConfigForCurrentUser"
    );
    assert!(!wire::x64::runtime::import_supported(
        "kernel32.dll",
        "WinHttpOpen"
    ));
}

#[test]
fn win64_sspi_tables_are_stable_guest_pointers_and_never_advertise_unavailable_security() {
    let (mut mem, mut cpu, mut runtime) = startup();
    runtime.install_gateways(Default::default());
    let text = STACK + 0x200;
    mem.write(text, b"secur32.dll\0").unwrap();
    let module = startup_call(&mut mem, &mut cpu, &mut runtime, "LoadLibraryA", &[text]);
    assert_ne!(module, 0);
    let mut tables = Vec::new();
    for suffix in ['A', 'W'] {
        let name = format!("InitSecurityInterface{suffix}");
        mem.write(text, format!("{name}\0").as_bytes()).unwrap();
        let gateway = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[module, text],
        );
        assert!(gateway > u32::MAX as u64);
        assert_eq!(runtime.gateways[&gateway], format!("secur32.dll!{name}"));
        let table = startup_dll_call(&mut mem, &mut cpu, &mut runtime, "secur32.dll", &name, &[]);
        assert!(table > u32::MAX as u64);
        assert_eq!(mem.read_int(table, 4).unwrap(), 1);
        assert_eq!(mem.read_int(table + 4, 4).unwrap(), 0);
        assert!(mem.write_int(table, 4, 99).is_err());
        for offset in [40, 144, 152, 184] {
            assert_eq!(mem.read_int(table + offset, 8).unwrap(), 0);
        }
        for (offset, function) in [
            (8, format!("EnumerateSecurityPackages{suffix}")),
            (24, format!("AcquireCredentialsHandle{suffix}")),
            (48, format!("InitializeSecurityContext{suffix}")),
            (128, "FreeContextBuffer".into()),
            (136, format!("QuerySecurityPackageInfo{suffix}")),
            (200, "EncryptMessage".into()),
            (208, "DecryptMessage".into()),
            (216, format!("SetContextAttributes{suffix}")),
        ] {
            let pointer = mem.read_int(table + offset, 8).unwrap();
            assert!(pointer > u32::MAX as u64);
            assert_eq!(
                runtime.gateways[&pointer],
                format!("secur32.dll!{function}")
            );
        }
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "secur32.dll",
                "FreeContextBuffer",
                &[table]
            ),
            0x80090301
        );
        let heap = startup_call(&mut mem, &mut cpu, &mut runtime, "GetProcessHeap", &[]);
        cpu.regs[4] = STACK + 0x1f88;
        mem.write_int(cpu.regs[4], 8, BASE).unwrap();
        cpu.regs[1] = heap;
        cpu.regs[2] = 0;
        cpu.regs[8] = table;
        assert!(runtime
            .dispatch("kernel32.dll", "HeapFree", &mut cpu, &mut mem)
            .is_err());
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut runtime, "secur32.dll", &name, &[]),
            table
        );
        tables.push(table);
    }
    assert_ne!(tables[0], tables[1]);
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "SetLastError",
        &[0x87654321],
    );
    let output = STACK + 0x400;
    mem.write(output, &[0xaa; 32]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "secur32.dll",
            "EnumerateSecurityPackagesW",
            &[output, output + 8]
        ),
        0
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 0xaaaaaaaa00000000);
    assert_eq!(mem.read_int(output + 8, 8).unwrap(), 0);
    mem.write(text, b"Schannel\0").unwrap();
    mem.write(output, &[0xbb; 32]).unwrap();
    for (name, args) in [
        ("QuerySecurityPackageInfoA", vec![text, output]),
        (
            "AcquireCredentialsHandleA",
            vec![0, text, 2, 0, 0, 0, 0, output, output + 16],
        ),
        (
            "InitializeSecurityContextA",
            vec![
                output,
                0,
                text,
                0,
                0,
                0,
                0,
                0,
                output,
                output,
                output + 16,
                output + 24,
            ],
        ),
        ("EncryptMessage", vec![output, 0, output, 0]),
        ("DecryptMessage", vec![output, output, 0, output + 24]),
        ("FreeContextBuffer", vec![output]),
    ] {
        let expected = if name.starts_with("QuerySecurityPackage") || name.starts_with("Acquire") {
            0x80090305
        } else {
            0x80090301
        };
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut runtime, "secur32.dll", name, &args),
            expected
        );
        let mut bytes = [0; 32];
        mem.read(output, &mut bytes).unwrap();
        assert_eq!(bytes, [0xbb; 32]);
    }
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        0x87654321
    );
    // A bad second output must not zero the first output or commit the call.
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = output;
    cpu.regs[2] = STACK + 0x1ffd;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch(
            "secur32.dll",
            "EnumerateSecurityPackagesA",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(mem.read_int(output, 4).unwrap(), 0xbbbbbbbb);
}

#[test]
fn amd64_guest_calls_the_sspi_function_table_across_single_instruction_budgets() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x680000000;
    const DATA: u64 = CODE + 4096;
    let dir = std::env::temp_dir().join(format!("wire-sspi64-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.exe"), fixture()).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
    p.memory
        .write(DATA, b"C:\\Windows\\System32\\secur32.dll\0")
        .unwrap();
    let module = startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "LoadLibraryExA",
        &[DATA, 0, 8],
    );
    assert_ne!(module, 0);
    assert!(!dir.join("Windows/System32/secur32.dll").exists());
    p.memory.write(DATA, b"InitSecurityInterfaceW\0").unwrap();
    let init = startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "GetProcAddress",
        &[module, DATA],
    );
    // Restore the actual process return sentinel, replaced by the direct-call helper.
    p.memory
        .write_int(p.cpu.regs[4], 8, 0x7ffe10000000)
        .unwrap();
    let mut code = vec![0x48, 0x83, 0xec, 0x28, 0x48, 0xb8];
    code.extend(init.to_le_bytes());
    code.extend([0xff, 0xd0, 0x48, 0x89, 0xc3, 0x48, 0xb9]); // RBX=table, RCX=count
    code.extend((DATA + 0x100).to_le_bytes());
    code.extend([0x48, 0xba]);
    code.extend((DATA + 0x108).to_le_bytes());
    code.extend([0xff, 0x53, 0x08, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
    p.memory.write(DATA + 0x100, &[0xcc; 16]).unwrap();
    p.memory.write(CODE, &code).unwrap();
    p.cpu.rip = CODE;
    loop {
        match p.run(p.cpu.work + 1).unwrap() {
            Outcome::Budget => assert!(p.cpu.work < 100),
            result => {
                assert_eq!(result, Outcome::Returned(0));
                break;
            }
        }
    }
    assert_eq!(p.memory.read_int(DATA + 0x100, 4).unwrap(), 0);
    assert_eq!(p.memory.read_int(DATA + 0x104, 4).unwrap(), 0xcccccccc);
    assert_eq!(p.memory.read_int(DATA + 0x108, 8).unwrap(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn win64_winsock_negotiates_versions_checks_full_output_and_balances_ordinal_startup_refs() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x200;
    let mut call = |name: &str, args: &[u64]| {
        startup_dll_call(&mut mem, &mut cpu, &mut runtime, "Ws2_32.dll", name, args)
    };
    call("#112", &[0x1234567887654321]);
    assert_eq!(call("WSAGetLastError", &[]), 0x87654321);
    assert_eq!(call("#115", &[0x0202, 0]), 10014);
    assert_eq!(call("WSAStartup", &[0, 0]), 10092);
    assert_eq!(call("#111", &[]), 0x87654321);
    assert_eq!(call("#116", &[]), u32::MAX as u64);
    assert_eq!(call("#111", &[]), 10093);

    // Every successful startup owns a process reference, independent of version.
    for (request, negotiated) in [
        (0x0101u16, 0x0101u16),
        (0x0001, 0x0001),
        (0x0301, 0x0101),
        (0x0202, 0x0202),
        (0x0102, 0x0102),
        (0x0302, 0x0202),
        (0x0003, 0x0202),
    ] {
        mem.write(output, &[0xa5; 416]).unwrap();
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "ws2_32.dll",
                "#115",
                &[0x1234000000000000 | request as u64, output]
            ),
            0
        );
        assert_eq!(mem.read_int(output, 2).unwrap(), negotiated as u64);
        assert_eq!(mem.read_int(output + 2, 2).unwrap(), 0x0202);
        assert_eq!(
            mem.read_int(output + 4, 2).unwrap(),
            if request as u8 == 1 { 32767 } else { 0 }
        );
        assert_eq!(
            mem.read_int(output + 6, 2).unwrap(),
            if request as u8 == 1 { 65467 } else { 0 }
        );
        assert_eq!(mem.read_int(output + 8, 8).unwrap(), 0xa5a5a5a5a5a5a5a5);
        let mut status = [0; 8];
        mem.read(output + 273, &mut status).unwrap();
        assert_eq!(&status, b"Running\0");
        let mut tail = [0; 14];
        mem.read(output + 402, &mut tail).unwrap();
        assert_eq!(tail, [0xa5; 14]);
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ws2_32.dll",
            "WSAGetLastError",
            &[]
        ),
        10093
    );

    // A checked fault cannot overwrite an initial field or increment references.
    let bad = STACK + 4096 - 200;
    mem.write(bad, &[0xcc; 200]).unwrap();
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    cpu.regs[4] = STACK + 0x188;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = 0x0202;
    cpu.regs[2] = bad;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("ws2_32.dll", "#115", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    let mut preserved = [0; 200];
    mem.read(bad, &mut preserved).unwrap();
    assert_eq!(preserved, [0xcc; 200]);
    mem.protect(STACK + 4096, 4096, READ | WRITE).unwrap();
    // Invalid version writes negotiation fields, but owns no reference.
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ws2_32.dll",
            "WSAStartup",
            &[0x0200, output]
        ),
        10092
    );
    assert_eq!(mem.read_int(output, 2).unwrap(), 0x0202);
    for _ in 0..7 {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "ws2_32.dll",
                "WSACleanup",
                &[]
            ),
            0
        );
    }
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ws2_32.dll",
            "WSACleanup",
            &[]
        ),
        u32::MAX as u64
    );

    runtime.register_provider("ws2_32.dll");
    mem.write(output, b"ws2_32.dll\0").unwrap();
    let module = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetModuleHandleA",
        &[output],
    );
    mem.write(output, b"WSAStartup\0").unwrap();
    let named = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetProcAddress",
        &[module, output],
    );
    let ordinal = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetProcAddress",
        &[module, 115],
    );
    assert_eq!(named, ordinal);
    assert_eq!(ordinal, runtime.register("ws2_32.dll", "#115"));
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetProcAddress",
            &[module, 65535]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        127
    );
}
