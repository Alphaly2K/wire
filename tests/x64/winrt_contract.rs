//! Independent HSTRING lifetime, lookup, fault and activation baseline.
use super::*;

const EXPORTS: [&str; 11] = [
    "WindowsCreateString",
    "WindowsCreateStringReference",
    "WindowsDeleteString",
    "WindowsDuplicateString",
    "WindowsGetStringLen",
    "WindowsIsStringEmpty",
    "WindowsGetStringRawBuffer",
    "WindowsStringHasEmbeddedNull",
    "WindowsCompareStringOrdinal",
    "RoGetActivationFactory",
    "RoActivateInstance",
];

fn call(
    mem: &mut Memory,
    cpu: &mut Cpu64,
    rt: &mut wire::x64::runtime::Runtime64,
    name: &str,
    args: &[u64],
) -> u64 {
    startup_dll_call(mem, cpu, rt, "combase.dll", name, args)
}

fn fault(
    mem: &mut Memory,
    cpu: &mut Cpu64,
    rt: &mut wire::x64::runtime::Runtime64,
    name: &str,
    args: &[u64],
) -> String {
    cpu.regs[4] = STACK + 0x1f88;
    assert_eq!(mem.read_int(cpu.regs[4], 8).unwrap(), BASE);
    for (i, &value) in args.iter().enumerate() {
        cpu.regs[[1, 2, 8, 9][i]] = value;
    }
    let before = cpu.clone();
    let calls = rt.api_calls;
    let error = rt.dispatch("combase.dll", name, cpu, mem).unwrap_err();
    assert_eq!(*cpu, before);
    assert_eq!(rt.api_calls, calls);
    error
}

#[test]
fn winrt_contract_keeps_static_providers_and_dynamic_apiset_remapping() {
    let (mut mem, mut cpu, mut rt) = startup();
    let string_api = "api-ms-win-core-winrt-string-l1-1-0.dll";
    let activation_api = "api-ms-win-core-winrt-l1-1-0.dll";
    for dll in [
        "combase.dll",
        string_api,
        activation_api,
        "kernel32.dll",
        "kernelbase.dll",
        "ole32.dll",
        "user32.dll",
    ] {
        rt.register_provider(dll);
        let text = STACK + 0x100;
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "GetModuleHandleA", &[text]);
        assert_ne!(module, 0);
        for name in EXPORTS {
            let windows = name.starts_with("Windows");
            let imported = dll == "combase.dll"
                || (dll == string_api && windows)
                || (dll == activation_api && !windows);
            assert_eq!(
                wire::x64::runtime::import_supported(dll, name),
                imported,
                "{dll}!{name}"
            );
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            let gateway = startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "GetProcAddress",
                &[module, text],
            );
            // Both WinRT API sets historically remap to the combase provider.
            if dll == "combase.dll" || dll == string_api || dll == activation_api {
                assert_ne!(gateway, 0, "{dll}!{name}");
                assert_eq!(rt.gateways[&gateway], format!("combase.dll!{name}"));
            } else {
                assert_eq!(gateway, 0, "{dll}!{name}");
            }
        }
    }
}

#[test]
fn winrt_contract_rereads_reference_headers_and_keeps_owned_metadata() {
    let (mut mem, mut cpu, mut rt) = startup();
    let source = STACK + 0x100;
    let header = source + 0x100;
    let output = header + 0x100;
    mem.write(source, &[0x41, 0, 0, 0, 0, 0]).unwrap();
    mem.write(header, &[0x55; 24]).unwrap();
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "WindowsCreateStringReference",
            &[source, 0xdeadbeef_00000002, header, output]
        ),
        0
    );
    assert_eq!(mem.read_int(header + 8, 8).unwrap(), 0x5555555555555555);
    mem.write_int(header + 4, 4, 1).unwrap();
    mem.write_int(header + 16, 8, source + 2).unwrap();
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "WindowsGetStringLen",
            &[header]
        ),
        1
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "WindowsGetStringRawBuffer",
            &[header, 0]
        ),
        source + 2
    );
    mem.write_int(header, 4, 0).unwrap();
    assert!(fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "WindowsGetStringLen",
        &[header]
    )
    .contains("invalid Win64 HSTRING reference header"));
    assert!(fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "WindowsGetStringLen",
        &[header & 0xffff_ffff]
    )
    .contains("unmapped"));

    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "WindowsCreateString",
            &[source, 2, output]
        ),
        0
    );
    let owned = mem.read_int(output, 8).unwrap();
    mem.protect(owned, 4096, READ | WRITE).unwrap();
    mem.write_int(owned, 8, 0).unwrap();
    mem.write_int(owned + 16, 8, 0).unwrap();
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "WindowsGetStringLen", &[owned]),
        2
    );
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "WindowsGetStringRawBuffer",
            &[owned, 0]
        ),
        owned + 32
    );
    assert_eq!(
        call(&mut mem, &mut cpu, &mut rt, "WindowsDeleteString", &[owned]),
        0
    );
    assert!(mem.read_int(owned, 1).is_err());
}

#[test]
fn winrt_contract_keeps_hresult_output_precedence_and_last_error() {
    let (mut mem, mut cpu, mut rt) = startup();
    let source = STACK + 0x100;
    let header = source + 0x100;
    let output = header + 0x100;
    mem.write(source, &[0x41, 0, 0x42, 0, 0, 0]).unwrap();
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
    for (name, args, expected) in [
        ("WindowsCreateString", vec![0, 1, output], 0x80004003),
        (
            "WindowsCreateString",
            vec![source, 0x400001, output],
            0x8007000e,
        ),
        ("WindowsCreateString", vec![source, 0x400001, 0], 0x80070057),
        (
            "WindowsCreateStringReference",
            vec![source, 1, header, output],
            0x80070057,
        ),
        (
            "WindowsCreateStringReference",
            vec![0, 0, 0, output],
            0x80070057,
        ),
        ("WindowsDuplicateString", vec![0xdead0000, 0], 0x80070057),
        (
            "WindowsStringHasEmbeddedNull",
            vec![0xdead0000, 0],
            0x80070057,
        ),
        (
            "WindowsCompareStringOrdinal",
            vec![0xdead0000, 0, 0],
            0x80070057,
        ),
    ] {
        mem.write_int(output, 8, 0x5555555555555555).unwrap();
        assert_eq!(
            call(&mut mem, &mut cpu, &mut rt, name, &args),
            expected,
            "{name}"
        );
        assert_eq!(mem.read_int(output, 8).unwrap(), 0x5555555555555555);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            123
        );
    }
    let bad = STACK + 4092;
    mem.protect(STACK + 4096, 4096, READ).unwrap();
    let memory = mem.memory_status();
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "WindowsCreateString",
        &[source, 0x400001, bad],
    );
    assert_eq!(mem.memory_status(), memory);
    mem.protect(STACK + 4096, 4096, READ | WRITE).unwrap();
    // Publishing through the return slot uses the gateway's saved frame.
    let return_slot = STACK + 0x1f88;
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "WindowsCreateStringReference",
            &[source, 2, header, return_slot]
        ),
        0
    );
    assert_eq!(mem.read_int(return_slot, 8).unwrap(), header);
    assert_eq!(cpu.rip, BASE);
}

#[test]
fn winrt_contract_keeps_activation_output_trace_and_pointer_validation() {
    let (mut mem, mut cpu, mut rt) = startup();
    let source = STACK + 0x100;
    let header = source + 0x100;
    let output = header + 0x100;
    let iid = output + 0x100;
    mem.write(source, &[0x41, 0, 0, 0]).unwrap();
    call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "WindowsCreateStringReference",
        &[source, 1, header, output],
    );
    for (name, args, expected) in [
        (
            "RoGetActivationFactory",
            vec![header, 0, output],
            0x80070057,
        ),
        ("RoGetActivationFactory", vec![header, iid, 0], 0x80070057),
        (
            "RoGetActivationFactory",
            vec![header, iid, output],
            0x80040154,
        ),
        ("RoActivateInstance", vec![header, output], 0x80040154),
    ] {
        mem.write_int(output, 8, 0x5555555555555555).unwrap();
        assert_eq!(call(&mut mem, &mut cpu, &mut rt, name, &args), expected);
        assert_eq!(mem.read_int(output, 8).unwrap(), 0x5555555555555555);
    }
    assert!(rt
        .trace
        .iter()
        .any(|entry| entry.contains("Win64 WinRT class=\"A\": no registered activation factory")));
    let trace: Vec<_> = rt.trace.iter().map(ToString::to_string).collect();
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "RoGetActivationFactory",
        &[header, 0xdead0000, output],
    );
    assert_eq!(
        rt.trace.iter().map(ToString::to_string).collect::<Vec<_>>(),
        trace
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 0x5555555555555555);
}
