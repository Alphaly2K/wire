//! Independent apartment, factory, task allocation and provider baseline.
use super::*;

#[test]
fn com_core_keeps_win64_apartment_reference_kinds_widths_and_provider_lookup() {
    let (mut mem, mut cpu, mut rt) = startup();
    let providers = [
        "kernel32.dll",
        "kernelbase.dll",
        "ole32.dll",
        "combase.dll",
        "api-ms-win-core-winrt-l1-1-0.dll",
    ];
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
    for dll in providers {
        startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
        let sequence: &[(&str, &[u64], u64)] = &[
            ("CoInitializeEx", &[0, 0xabcdef12_00000002], 0),
            ("CoInitialize", &[0], 1),
            ("RoInitialize", &[0xabcdef12_00000000], 1),
            ("CoInitializeEx", &[0, 0], 0x80010106),
            ("CoUninitialize", &[], 0),
            ("CoUninitialize", &[], 0),
            ("CoUninitialize", &[], 0),
            ("CoInitializeEx", &[0, 0], 0x80010106),
            ("RoUninitialize", &[], 0),
            ("CoInitializeEx", &[0, 0xc], 0),
            ("RoInitialize", &[1], 1),
            ("CoUninitialize", &[], 0),
            ("CoInitialize", &[0], 0x80010106),
            ("RoUninitialize", &[], 0),
            ("CoInitialize", &[0x100000000], 0x80070057),
            ("CoInitializeEx", &[0, 1], 0x80070057),
            ("RoInitialize", &[2], 0x80070057),
            ("CoInitializeEx", &[0, 2], 0),
            ("CoUninitialize", &[], 0),
        ];
        for &(name, args, expected) in sequence {
            assert_eq!(
                startup_dll_call(&mut mem, &mut cpu, &mut rt, dll, name, args),
                expected,
                "{dll}!{name}"
            );
        }
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            123
        );
        let pointer = STACK + 0x100;
        rt.register_provider(dll);
        mem.write(pointer, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[pointer]);
        for name in [
            "CoInitialize",
            "CoInitializeEx",
            "CoUninitialize",
            "RoInitialize",
            "RoUninitialize",
            "CoCreateInstance",
            "CoTaskMemAlloc",
            "CoTaskMemFree",
        ] {
            mem.write(pointer, format!("{name}\0").as_bytes()).unwrap();
            let gateway = startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "GetProcAddress",
                &[module, pointer],
            );
            assert_eq!(gateway != 0, !dll.starts_with("kernel"), "{dll}!{name}");
        }
    }
    for dll in ["ole32.dll", "combase.dll", "user32.dll"] {
        for name in ["OleInitialize", "OleUninitialize"] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
        }
    }
    for name in [
        "CoInitialize",
        "CoInitializeEx",
        "CoUninitialize",
        "RoInitialize",
        "RoUninitialize",
        "CoCreateInstance",
        "CoTaskMemAlloc",
        "CoTaskMemFree",
    ] {
        assert!(!wire::x64::runtime::import_supported("user32.dll", name));
    }
}

fn failing_call(
    rt: &mut wire::x64::runtime::Runtime64,
    mem: &mut Memory,
    cpu: &mut Cpu64,
    name: &str,
    args: &[u64],
) -> String {
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    for (i, &value) in args.iter().enumerate() {
        if i < 4 {
            cpu.regs[[1, 2, 8, 9][i]] = value;
        } else {
            mem.write_int(cpu.regs[4] + 40 + (i as u64 - 4) * 8, 8, value)
                .unwrap();
        }
    }
    let before = cpu.clone();
    let calls = rt.api_calls;
    let error = rt.dispatch("ole32.dll", name, cpu, mem).unwrap_err();
    assert_eq!(*cpu, before);
    assert_eq!(rt.api_calls, calls);
    error
}

#[test]
fn com_core_keeps_win64_factory_output_preflight_alias_and_hresult_order() {
    let (mut mem, mut cpu, mut rt) = startup();
    let clsid = STACK + 0x100;
    let iid = clsid + 16;
    let output = clsid + 32;
    mem.write(clsid, &[0x55; 40]).unwrap();
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
    for (args, expected) in [
        ([0, 0, 1, iid, output], 0x80070057),
        ([clsid, 0, 1, iid, output], 0x800401f0),
    ] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "ole32.dll",
                "CoCreateInstance",
                &args
            ),
            expected
        );
    }
    assert_eq!(mem.read_int(output, 8).unwrap(), 0);
    startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "ole32.dll",
        "CoInitialize",
        &[0],
    );
    for context in [0, 0xabcdef12_00000000, 1, 0xabcdef12_00000001] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "ole32.dll",
                "CoCreateInstance",
                &[clsid, u64::MAX, context, iid, output]
            ),
            if context as u32 == 0 {
                0x80070057
            } else {
                0x80040154
            }
        );
    }
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        123
    );
    let trace = rt.trace.len();
    mem.virtual_protect(STACK, 4096, 4 | wire::mem::PAGE_GUARD)
        .unwrap();
    assert!(failing_call(
        &mut rt,
        &mut mem,
        &mut cpu,
        "CoCreateInstance",
        &[clsid, 0, 1, iid, output]
    )
    .contains("guard-page"));
    assert_eq!(rt.trace.len(), trace);
    mem.write_int(output, 8, 0x1234).unwrap();
    failing_call(
        &mut rt,
        &mut mem,
        &mut cpu,
        "CoCreateInstance",
        &[0xdead0000, 0, 1, iid, output],
    );
    assert_eq!(mem.read_int(output, 8).unwrap(), 0x1234);
    assert_eq!(rt.trace.len(), trace);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "ole32.dll",
            "CoCreateInstance",
            &[clsid, 0, 1, iid, STACK + 0x1f88]
        ),
        0x80040154
    );
    assert_eq!(mem.read_int(STACK + 0x1f88, 8).unwrap(), 0);
    assert_eq!(cpu.rip, BASE);
    // Bad return frame must not initialize an apartment.
    startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "ole32.dll",
        "CoUninitialize",
        &[],
    );
    cpu.regs[4] = 0xdead0000;
    cpu.regs[1] = 0;
    assert!(rt
        .dispatch("ole32.dll", "CoInitialize", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "ole32.dll",
            "CoInitializeEx",
            &[0, 0]
        ),
        0
    );
}

#[test]
fn com_core_keeps_win64_task_ownership_full_size_and_checked_free() {
    let (mut mem, mut cpu, mut rt) = startup();
    let output = STACK + 0x100;
    let p = startup_dll_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "ole32.dll",
        "CoTaskMemAlloc",
        &[0],
    );
    assert_ne!(p, 0);
    assert_eq!(mem.read_int(p, 1).unwrap(), 0);
    assert!(failing_call(
        &mut rt,
        &mut mem,
        &mut cpu,
        "CoTaskMemFree",
        &[p | 0x8000000000000000]
    )
    .contains("invalid Win64 task allocation"));
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "ole32.dll",
            "CoTaskMemFree",
            &[p]
        ),
        0
    );
    assert!(mem.read_int(p, 1).is_err());
    failing_call(&mut rt, &mut mem, &mut cpu, "CoTaskMemFree", &[p]);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "combase.dll",
            "CoTaskMemFree",
            &[0]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "ole32.dll",
            "CoTaskMemAlloc",
            &[0x100000000]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        8
    );
    // A GUID string is owned by the same task allocator.
    mem.write(output, &[0; 16]).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "ole32.dll",
            "StringFromCLSID",
            &[output, output + 32]
        ),
        0
    );
    let p = mem.read_int(output + 32, 8).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "ole32.dll",
            "CoTaskMemFree",
            &[p]
        ),
        0
    );
    assert!(mem.read_int(p, 1).is_err());
    let local = startup_call(&mut mem, &mut cpu, &mut rt, "LocalAlloc", &[0, 16]);
    failing_call(&mut rt, &mut mem, &mut cpu, "CoTaskMemFree", &[local]);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "LocalFree", &[local]),
        0
    );
}

#[test]
fn com_core_keeps_win64_apartments_isolated_across_real_thread_switches() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x592000000;
    const WORKER: u64 = CODE + 4096;
    const DATA: u64 = CODE + 8192;
    fn mov(code: &mut Vec<u8>, register: u8, value: u64) {
        code.extend([0x48 | (register >> 3), 0xb8 | (register & 7)]);
        code.extend(value.to_le_bytes());
    }
    fn call(p: &mut Process64, code: &mut Vec<u8>, name: &str, args: &[u64]) {
        for (n, &value) in args.iter().enumerate() {
            mov(code, if n < 4 { [1, 2, 8, 9][n] } else { 0 }, value);
            if n >= 4 {
                code.extend([0x48, 0x89, 0x44, 0x24, 32 + (n as u8 - 4) * 8]);
            }
        }
        mov(code, 0, p.runtime.register("kernelbase.dll", name));
        code.extend([0xff, 0xd0]);
    }
    fn store(code: &mut Vec<u8>, at: u64) {
        mov(code, 7, at);
        code.extend([0x48, 0x89, 0x07]);
    }
    let path = std::env::temp_dir().join(format!("wire-com-core64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for slice in [1, 100000] {
        let mut p = Process64::load(&path, false).unwrap();
        p.memory
            .map(CODE, 3 * 4096, READ | WRITE | EXECUTE)
            .unwrap();
        let mut main = vec![0x48, 0x83, 0xec, 0x38];
        call(&mut p, &mut main, "CoInitialize", &[0]);
        store(&mut main, DATA);
        call(&mut p, &mut main, "CreateThread", &[0, 0, WORKER, 0, 0, 0]);
        main.extend([0x48, 0x89, 0xc1]);
        mov(&mut main, 2, u32::MAX as u64);
        call(&mut p, &mut main, "WaitForSingleObject", &[]);
        call(&mut p, &mut main, "CoInitialize", &[0]);
        store(&mut main, DATA + 8);
        call(&mut p, &mut main, "CoInitializeEx", &[0, 0]);
        store(&mut main, DATA + 16);
        call(&mut p, &mut main, "CoUninitialize", &[]);
        call(&mut p, &mut main, "CoUninitialize", &[]);
        call(&mut p, &mut main, "CoInitializeEx", &[0, 0]);
        store(&mut main, DATA + 24);
        main.extend([0x48, 0x83, 0xc4, 0x38, 0xc3]);
        let mut worker = vec![0x48, 0x83, 0xec, 0x28];
        for (i, (name, args)) in [
            ("CoInitializeEx", vec![0, 0]),
            ("RoInitialize", vec![1]),
            ("CoUninitialize", vec![]),
            ("CoInitialize", vec![0]),
            ("RoUninitialize", vec![]),
            ("CoInitialize", vec![0]),
        ]
        .into_iter()
        .enumerate()
        {
            call(&mut p, &mut worker, name, &args);
            store(&mut worker, DATA + 32 + i as u64 * 8);
        }
        worker.extend([0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(CODE, &main).unwrap();
        p.memory.write(WORKER, &worker).unwrap();
        p.cpu.rip = CODE;
        loop {
            match p.run(p.cpu.work + slice).unwrap() {
                Outcome::Budget => assert!(p.cpu.work < 20000),
                result => {
                    assert_eq!(result, Outcome::Returned(0));
                    break;
                }
            }
        }
        for (i, value) in [0, 1, 0x80010106, 0, 0, 1, 0, 0x80010106, 0, 0]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                p.memory.read_int(DATA + i as u64 * 8, 8).unwrap(),
                value,
                "slice={slice} slot={i}"
            );
        }
    }
    std::fs::remove_file(path).unwrap();
}
