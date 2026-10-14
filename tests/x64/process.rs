//! Existing behavioral tests grouped by process; expectations remain independent of API catalogs.

use super::*;

#[test]
fn amd64_cli_probe_executes_high_entry_and_fails_on_unresolved_import() {
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("wire-x64-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("probe.exe");
    let mut b = fixture();
    std::fs::write(&path, &b).unwrap();
    let binary = env!("CARGO_BIN_EXE_wire");
    let output = Command::new(binary)
        .args(["probe64", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("entry returned RAX=0x123456789abcdef0; retired=3"));
    // call [RIP-relative IAT] reaches the unresolved import marker after a real CALL.
    put32(&mut b, 0x98 + 120, 0x1140);
    put32(&mut b, 0x98 + 124, 40);
    put32(&mut b, 0x340, 0x1180);
    put32(&mut b, 0x34c, 0x11c0);
    put32(&mut b, 0x350, 0x1190);
    put64(&mut b, 0x380, 0x11d0);
    b[0x3c0..0x3cd].copy_from_slice(b"kernel32.dll\0");
    b[0x3d2..0x3de].copy_from_slice(b"ExitProcess\0");
    b[0x200..0x207].copy_from_slice(&[0xff, 0x15, 0x8a, 1, 0, 0, 0xc3]);
    std::fs::write(&path, &b).unwrap();
    let output = Command::new(binary)
        .args(["probe64", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("import boundary: kernel32.dll!ExitProcess; retired=1"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not run the game"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn win64_api_inventory_is_opt_in_counts_calls_and_keeps_lookup_provider() {
    let (mut mem, mut cpu, mut runtime) = startup();
    startup_call(&mut mem, &mut cpu, &mut runtime, "GetCurrentThreadId", &[]);
    assert!(runtime.api_inventory_tsv().is_none());
    runtime.enable_api_inventory();
    for _ in 0..2 {
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetCurrentThreadId", &[]);
    }
    let symbol = STACK + 0x200;
    mem.write(symbol, b"Export\tWith\\Escape\0").unwrap();
    let mut exports = wire::x64::pe::Exports64::default();
    exports
        .names
        .insert("Export\tWith\\Escape".into(), BASE + 0x2345);
    exports.ordinals.insert(7, BASE + 0x2345);
    runtime.register_module("Guest.dll", BASE, exports);
    for name in [symbol, 7] {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetProcAddress",
                &[BASE, name]
            ),
            BASE + 0x2345
        );
    }
    // A checked frame failure must not become a completed call.
    cpu.regs[4] = 0;
    assert!(runtime
        .dispatch("kernel32.dll", "GetCurrentThreadId", &mut cpu, &mut mem)
        .is_err());
    let tsv = runtime.api_inventory_tsv().unwrap();
    assert!(tsv.contains("completed\tkernel32.dll\tGetCurrentThreadId\t2\n"));
    assert!(tsv.contains("lookup\tguest.dll\tExport\\tWith\\\\Escape\t1\n"));
    assert!(tsv.contains("lookup\tguest.dll\t#7\t1\n"));
}

#[test]
fn win64_api_inventory_retains_gateway_failure_after_process_error() {
    let dir = std::env::temp_dir().join(format!("wire-api-inventory-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("probe.exe");
    std::fs::write(&exe, fixture()).unwrap();
    let mut process = wire::x64::process::Process64::load(&exe, false).unwrap();
    process.runtime.enable_api_inventory();
    process.cpu.rip = process.runtime.register("hid.dll", "HidD_UnknownForTest");
    assert!(process
        .run(10)
        .unwrap_err()
        .contains("unimplemented Win64 import"));
    let tsv = process.runtime.api_inventory_tsv().unwrap();
    assert!(tsv.contains("failed\thid.dll\tHidD_UnknownForTest\t1\n"));
    assert!(!tsv.contains("completed\thid.dll"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn win64_gateway_dispatch_preserves_custom_addresses_and_executes_guest_gaps() {
    use wire::x64::process::{Outcome, Process64};
    let dir = std::env::temp_dir().join(format!("wire-gateway-bounds-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("probe.exe");
    std::fs::write(&exe, fixture()).unwrap();
    let mut process = Process64::load(&exe, false).unwrap();
    let first = BASE + 0x1100;
    let last = BASE + 0x1200;
    process
        .runtime
        .install_gateways(std::collections::BTreeMap::from([
            (first, "kernel32.dll!GetCurrentThreadId".into()),
            (last, "kernel32.dll!GetCurrentThreadId".into()),
        ]));
    process
        .memory
        .protect(BASE + 0x1000, 4096, READ | WRITE | EXECUTE)
        .unwrap();
    // Instructions below, within and above the gateway envelope still execute.
    for at in [first - 16, first + 16, last + 16] {
        process.memory.write(at, &[0xb8, 42, 0, 0, 0]).unwrap();
    }
    process
        .memory
        .protect(BASE + 0x1000, 4096, READ | EXECUTE)
        .unwrap();
    for at in [first - 16, first + 16, last + 16] {
        process.cpu.rip = at;
        let retired = process.cpu.retired;
        assert_eq!(process.run(process.cpu.work + 1).unwrap(), Outcome::Budget);
        assert_eq!(process.cpu.regs[0], 42);
        assert_eq!(process.cpu.retired, retired + 1);
    }
    // Dynamic insertions extend both ends; dispatch precedes instruction fetch.
    let low = 0x8000;
    let high = 0x7fff00000000;
    for at in [low, high] {
        process
            .runtime
            .gateways
            .insert(at, "kernel32.dll!GetCurrentThreadId".into());
    }
    let rsp = process.cpu.regs[4];
    for at in [first, last, low, high] {
        process.cpu.rip = at;
        process.cpu.regs[4] = rsp;
        process.cpu.regs[0] = u64::MAX;
        process.memory.write_int(rsp, 8, first + 16).unwrap();
        let retired = process.cpu.retired;
        let calls = process.runtime.api_calls;
        assert_eq!(process.run(process.cpu.work + 1).unwrap(), Outcome::Budget);
        assert_eq!(process.cpu.regs[0], 1);
        assert_eq!(process.cpu.rip, first + 16);
        assert_eq!(process.cpu.retired, retired);
        assert_eq!(process.runtime.api_calls, calls + 1);
    }
    // A warmed register segment may approach a custom gateway, but must not
    // execute the instruction bytes registered at that interior address.
    process
        .memory
        .protect(BASE + 0x1000, 4096, READ | WRITE | EXECUTE)
        .unwrap();
    process.memory.write(first - 8, &[0x90; 24]).unwrap();
    process
        .memory
        .protect(BASE + 0x1000, 4096, READ | EXECUTE)
        .unwrap();
    process.cpu.rip = first - 8;
    process.cpu.regs[4] = rsp;
    process.cpu.rip = first;
    process.cpu.decode(&process.memory).unwrap();
    process.cpu.rip = first - 8;
    process.cpu.decode(&process.memory).unwrap();
    process.memory.write_int(rsp, 8, first + 16).unwrap();
    let retired = process.cpu.retired;
    let calls = process.runtime.api_calls;
    assert_eq!(process.run(process.cpu.work + 9).unwrap(), Outcome::Budget);
    assert_eq!(process.cpu.retired, retired + 8);
    assert_eq!(process.runtime.api_calls, calls + 1);
    assert_eq!(process.cpu.rip, first + 16);
    // A linked target can equal both ends of a singleton gateway envelope.
    // The preceding sixteen NOPs may be grouped, but the warmed target's
    // bytes must never execute in place of the registered API.
    process.runtime.gateways =
        std::collections::BTreeMap::from([(first, "kernel32.dll!GetCurrentThreadId".into())])
            .into();
    process
        .memory
        .protect(BASE + 0x1000, 4096, READ | WRITE | EXECUTE)
        .unwrap();
    process.memory.write(first - 16, &[0x90; 32]).unwrap();
    process
        .memory
        .protect(BASE + 0x1000, 4096, READ | EXECUTE)
        .unwrap();
    process.cpu.rip = first;
    process.cpu.decode(&process.memory).unwrap();
    process.cpu.rip = first - 16;
    process.cpu.decode(&process.memory).unwrap();
    process.cpu.regs[4] = rsp;
    process.memory.write_int(rsp, 8, first + 16).unwrap();
    let retired = process.cpu.retired;
    let calls = process.runtime.api_calls;
    assert_eq!(process.run(process.cpu.work + 17).unwrap(), Outcome::Budget);
    assert_eq!(process.cpu.retired, retired + 16);
    assert_eq!(process.runtime.api_calls, calls + 1);
    assert_eq!(process.cpu.rip, first + 16);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn amd64_vectored_software_exceptions_execute_guest_handlers_and_restore_full_context_across_budgets(
) {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x540000000;
    const DATA: u64 = CODE + 4096;
    const VALUE: u64 = 0x123456789abcdef0;
    fn mov(code: &mut Vec<u8>, register: u8, value: u64) {
        code.extend([0x48 | (register >> 3), 0xb8 | (register & 7)]);
        code.extend(value.to_le_bytes());
    }
    let path = std::env::temp_dir().join(format!("wire-veh64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for noncontinuable in [false, true] {
        let mut p = Process64::load(&path, false).unwrap();
        p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
        let mut original = p.cpu.clone();
        original.rip = CODE;
        original.xmm[0] = [0xa5; 16];
        original.fpu.cw = 0x0b7f;
        original.fpu.sw = 3 << 11;
        original.fpu.set_st_bits(0, 0x3fff8000000000000000);
        original.fpu.ip = CODE + 0x123;
        original.fpu.dp = DATA + 0x456;
        original.rflags = 0x602;
        let expected_fpu = original.fpu;
        let raise = p.runtime.register("kernel32.dll", "RaiseException");
        // Register in append order. Both callbacks actually run guest instructions.
        for target in [CODE + 0x200, CODE + 0x300] {
            assert_ne!(
                startup_call(
                    &mut p.memory,
                    &mut p.cpu,
                    &mut p.runtime,
                    "AddVectoredExceptionHandler",
                    &[0, target]
                ),
                0
            );
        }
        let mut main = vec![0x48, 0x83, 0xec, 0x28];
        for (r, v) in [
            (1, 0xe0400042),
            (2, noncontinuable as u64),
            (8, 2),
            (9, DATA + 256),
            (0, raise),
        ] {
            mov(&mut main, r, v);
        }
        main.extend([0xff, 0xd0]);
        mov(&mut main, 7, DATA);
        main.extend([0x48, 0x89, 0x07, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(CODE, &main).unwrap();
        p.memory
            .write_int(DATA + 256, 8, 0x1122334455667788)
            .unwrap();
        p.memory
            .write_int(DATA + 264, 8, 0x8877665544332211)
            .unwrap();
        p.memory.write_int(DATA + 128, 2, 0x077f).unwrap();
        // First callback reads the real EXCEPTION_RECORD and stores the argument,
        // then returns CONTINUE_SEARCH rather than ending dispatch.
        let mut first = vec![0x48, 0x8b, 0x01, 0x48, 0x8b, 0x50, 0x20];
        mov(&mut first, 7, DATA + 16);
        first.extend([0x48, 0x89, 0x17, 0x48, 0x8b, 0x41, 8]);
        mov(&mut first, 7, DATA + 24);
        first.extend([0x48, 0x89, 0x07, 0x31, 0xc0, 0xc3]);
        p.memory.write(CODE + 0x200, &first).unwrap();
        // Second callback edits the context's RAX and deliberately clobbers its
        // own XMM0/control word. Resuming must use the saved exception context.
        let mut second = vec![0x48, 0x8b, 0x41, 8];
        mov(&mut second, 2, VALUE);
        second.extend([0x48, 0x89, 0x50, 120, 0x0f, 0x57, 0xc0]);
        mov(&mut second, 7, DATA + 128);
        second.extend([0xd9, 0x2f, 0xb8, 0xff, 0xff, 0xff, 0xff, 0xc3]);
        p.memory.write(CODE + 0x300, &second).unwrap();
        p.cpu = original;
        loop {
            let budget = p.cpu.work + 1;
            match p.run(budget) {
                Ok(Outcome::Budget) => assert!(p.cpu.work < 200),
                Ok(result) => {
                    assert!(!noncontinuable);
                    assert_eq!(result, Outcome::Returned(VALUE));
                    assert_eq!(p.cpu.xmm[0], [0xa5; 16]);
                    assert_eq!(p.cpu.fpu, expected_fpu);
                    assert_eq!(p.cpu.rflags, 0x602);
                    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), VALUE);
                    break;
                }
                Err(error) => {
                    assert!(noncontinuable, "{error}");
                    assert!(error.contains("noncontinuable"));
                    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), 0);
                    break;
                }
            }
        }
        assert_eq!(p.memory.read_int(DATA + 16, 8).unwrap(), 0x1122334455667788);
        assert_eq!(
            p.stages
                .iter()
                .filter(|(s, _)| s == "vectored exception handler")
                .count(),
            2
        );
        let context = p.memory.read_int(DATA + 24, 8).unwrap();
        assert!(context > u32::MAX as u64);
        if !noncontinuable {
            assert!(p.memory.read_int(context, 1).is_err());
        }
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn amd64_same_frame_exception_unwind_executes_guest_language_and_termination_handlers() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x560000000;
    const DATA: u64 = CODE + 4096;
    const VALUE: u64 = 0xfedcba9876543210;
    fn mov(code: &mut Vec<u8>, r: u8, value: u64) {
        code.extend([0x48 | (r >> 3), 0xb8 | (r & 7)]);
        code.extend(value.to_le_bytes());
    }
    let path = std::env::temp_dir().join(format!("wire-seh64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for termination in [false, true] {
        let mut p = Process64::load(&path, false).unwrap();
        let mut original = p.cpu.clone();
        original.rip = CODE;
        p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
        let raise = p.runtime.register("kernel32.dll", "RaiseException");
        let unwind = p.runtime.register("kernel32.dll", "RtlUnwindEx");
        let mut main = vec![0x48, 0x83, 0xec, 0x28];
        for (r, v) in [(1, 0xe0410042), (2, 0), (8, 0), (9, 0), (0, raise)] {
            mov(&mut main, r, v);
        }
        main.extend([0xff, 0xd0]);
        // A returning RaiseException would write this marker. Target unwind must
        // instead transfer to the catch block with the requested 64-bit RAX.
        mov(&mut main, 7, DATA + 8);
        main.extend([0xc7, 0x07, 0xad, 0xde, 0, 0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
        let catch = main.len() as u64;
        mov(&mut main, 7, DATA);
        main.extend([0x48, 0x89, 0x07, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(CODE, &main).unwrap();
        // Guest handler dispatches using HandlerData, not a host-selected catch PC.
        let mut handler = vec![0x48, 0x83, 0xec, 0x38, 0xf7, 0x41, 4, 2, 0, 0, 0, 0x75, 0];
        let branch = handler.len() - 1;
        handler.extend([
            0x49, 0x89, 0xcc, 0x49, 0x89, 0xd5, 0x4d, 0x89, 0xc6, 0x4d, 0x89, 0xcf,
        ]);
        mov(&mut handler, 7, DATA + 16);
        handler.extend([0x4c, 0x89, 0x2f]);
        mov(&mut handler, 7, DATA + 32);
        handler.extend([
            0x4c, 0x89, 0x37, 0x49, 0x8b, 0x47, 0x38, 0x8b, 0x50, 0x10, 0x49, 0x03, 0x57, 8,
        ]);
        handler.extend([0x4c, 0x89, 0xe9, 0x4d, 0x89, 0xe0]);
        mov(&mut handler, 9, VALUE);
        handler.extend([
            0x4c, 0x89, 0x74, 0x24, 0x20, 0x48, 0xc7, 0x44, 0x24, 0x28, 0, 0, 0, 0,
        ]);
        mov(&mut handler, 0, unwind);
        handler.extend([0xff, 0xd0, 0xcc]); // Unwind never returns to this call site.
        let cleanup = handler.len();
        handler[branch] = u8::try_from(cleanup - branch - 1).unwrap();
        handler.extend([0x8b, 0x41, 4]);
        mov(&mut handler, 7, DATA + 24);
        handler.extend([0x89, 0x07, 0xb8, 1, 0, 0, 0, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
        p.memory.write(CODE + 0x200, &handler).unwrap();
        let table = DATA + 512;
        p.memory.write_int(table, 4, 0).unwrap();
        p.memory.write_int(table + 4, 4, main.len() as u64).unwrap();
        p.memory.write_int(table + 8, 4, 4096 + 544).unwrap();
        let mut metadata = [0; 32];
        metadata[..8].copy_from_slice(&[
            if termination { 0x19 } else { 9 },
            4,
            1,
            0,
            4,
            0x42,
            0,
            0,
        ]);
        put32(&mut metadata, 8, 0x200);
        put32(&mut metadata, 12, 1);
        put32(&mut metadata, 16, 4);
        put32(&mut metadata, 20, catch as u32);
        put32(&mut metadata, 24, 1);
        put32(&mut metadata, 28, catch as u32);
        p.memory.write(DATA + 544, &metadata).unwrap();
        assert_eq!(
            startup_call(
                &mut p.memory,
                &mut p.cpu,
                &mut p.runtime,
                "RtlAddFunctionTable",
                &[table, 1, CODE]
            ),
            1
        );
        p.cpu = original.clone();
        loop {
            match p.run(p.cpu.work + 1).unwrap() {
                Outcome::Budget => assert!(p.cpu.work < 500),
                result => {
                    assert_eq!(result, Outcome::Returned(VALUE));
                    break;
                }
            }
        }
        assert_eq!(p.memory.read_int(DATA, 8).unwrap(), VALUE);
        assert_eq!(p.memory.read_int(DATA + 8, 8).unwrap(), 0);
        assert_eq!(
            p.memory.read_int(DATA + 16, 8).unwrap(),
            original.regs[4] - 40
        );
        assert_eq!(
            p.memory.read_int(DATA + 24, 4).unwrap(),
            if termination { 0x22 } else { 0 }
        );
        assert_eq!(p.cpu.regs[4], original.regs[4] + 8);
        let context = p.memory.read_int(DATA + 32, 8).unwrap();
        assert!(p.memory.read_int(context, 1).is_err());
        assert_eq!(
            p.stages
                .iter()
                .filter(|(s, _)| s == "language exception handler")
                .count(),
            1
        );
        assert_eq!(
            p.stages
                .iter()
                .filter(|(s, _)| s == "target unwind handler")
                .count(),
            termination as usize
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn amd64_function_tables_lookup_real_metadata_and_execute_guest_lookup_callback_across_budgets() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x520000000;
    const DATA: u64 = CODE + 4096;
    fn mov(code: &mut Vec<u8>, reg: u8, value: u64) {
        code.extend([0x48 | (reg >> 3), 0xb8 | (reg & 7)]);
        code.extend(value.to_le_bytes());
    }
    let dir = std::env::temp_dir().join(format!("wire-unwind64-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut b = fixture();
    put32(&mut b, 0x98 + 112 + 3 * 8, 0x1100);
    put32(&mut b, 0x98 + 116 + 3 * 8, 12);
    put32(&mut b, 0x300, 0x1000);
    put32(&mut b, 0x304, 0x100e);
    put32(&mut b, 0x308, 0x11f0);
    b[0x3f0] = 1;
    std::fs::write(dir.join("main.exe"), b).unwrap();
    let mut p = Process64::load(&dir.join("main.exe"), false).unwrap();
    let original = p.cpu.clone();
    p.memory.map(CODE, 8192, READ | WRITE | EXECUTE).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlLookupFunctionEntry",
            &[BASE + 0x1004, DATA, 0]
        ),
        BASE + 0x1100
    );
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), BASE);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlDeleteFunctionTable",
            &[BASE + 0x1100]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlLookupFunctionEntry",
            &[BASE + 0x1500, DATA, 0]
        ),
        0
    );
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), 0);
    let handler = startup_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "AddVectoredExceptionHandler",
        &[1, CODE + 0x400],
    );
    assert_ne!(handler, 0);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "RemoveVectoredExceptionHandler",
            &[handler]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "RemoveVectoredExceptionHandler",
            &[handler]
        ),
        0
    );
    p.cpu.regs[4] = STACK + 0x1f88;
    p.memory.write_int(p.cpu.regs[4], 8, BASE).unwrap();
    p.cpu.regs[1] = 0xe0000001;
    p.cpu.regs[2] = 1;
    p.cpu.regs[8] = 0;
    p.cpu.regs[9] = 0;
    let before = p.cpu.clone();
    let error = p
        .runtime
        .dispatch("kernel32.dll", "RaiseException", &mut p.cpu, &mut p.memory)
        .unwrap_err();
    assert!(error.contains("code=0xe0000001"));
    assert_eq!(p.cpu.regs, before.regs);
    assert_eq!(p.cpu.rip, before.rip);
    let entry = DATA + 32;
    p.memory.write_int(entry, 4, 0x100).unwrap();
    p.memory.write_int(entry + 4, 4, 0x200).unwrap();
    p.memory.write_int(entry + 8, 4, 0x800).unwrap();
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlAddFunctionTable",
            &[entry, 1, CODE]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlAddFunctionTable",
            &[entry, 1, CODE]
        ),
        0
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlLookupFunctionEntry",
            &[CODE + 0x180, DATA, 0]
        ),
        entry
    );
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), CODE);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlDeleteFunctionTable",
            &[entry]
        ),
        1
    );
    let context = 0x123456789abcdef0;
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlInstallFunctionTableCallback",
            &[CODE | 3, CODE, 4096, CODE + 0x400, context, 0]
        ),
        1
    );
    let mut callback = Vec::new();
    mov(&mut callback, 0, DATA + 64);
    callback.extend([0x48, 0x89, 0x08, 0x48, 0x89, 0x50, 8]);
    mov(&mut callback, 0, entry);
    callback.push(0xc3);
    p.memory.write(CODE + 0x400, &callback).unwrap();
    let mut code = vec![0x48, 0x83, 0xec, 0x28];
    mov(&mut code, 1, CODE + 0x180);
    mov(&mut code, 2, DATA);
    mov(&mut code, 8, 0);
    mov(
        &mut code,
        0,
        p.runtime.register("ntdll.dll", "RtlLookupFunctionEntry"),
    );
    code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
    p.memory.write(CODE, &code).unwrap();
    p.cpu = original;
    p.cpu.rip = CODE;
    loop {
        let budget = p.cpu.work + 1;
        match p.run(budget).unwrap() {
            Outcome::Budget => assert!(p.cpu.work < 1000),
            value => {
                assert_eq!(value, Outcome::Returned(entry));
                break;
            }
        }
    }
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), CODE);
    assert_eq!(p.memory.read_int(DATA + 64, 8).unwrap(), CODE + 0x180);
    assert_eq!(p.memory.read_int(DATA + 72, 8).unwrap(), context);
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlDeleteFunctionTable",
            &[CODE | 3]
        ),
        1
    );
    assert_eq!(
        startup_dll_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "ntdll.dll",
            "RtlLookupFunctionEntry",
            &[CODE + 0x180, DATA, 0]
        ),
        0
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn win64_gateway_faults_preserve_cpu_and_outputs_and_heap_payloads_survive_realloc() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x100;
    mem.write_int(output, 8, 0x1122334455667788).unwrap();
    cpu.regs[1] = output;
    cpu.regs[4] = 0x123000;
    let before = cpu.clone();
    assert!(runtime
        .dispatch(
            "kernel32.dll",
            "GetSystemTimeAsFileTime",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(mem.read_int(output, 8).unwrap(), 0x1122334455667788);
    assert_eq!(runtime.api_calls, 0);
    cpu.regs[4] = STACK + 0x1fc8;
    cpu.regs[1] = STACK + 8190;
    let before = cpu.clone();
    assert!(runtime
        .dispatch(
            "kernel32.dll",
            "GetSystemTimeAsFileTime",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(runtime.api_calls, 0);
    let heap = startup_call(&mut mem, &mut cpu, &mut runtime, "GetProcessHeap", &[]);
    let ptr = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "HeapAlloc",
        &[heap, 8, 5000],
    );
    assert!(ptr > u32::MAX as u64);
    mem.write_int(ptr + 4094, 8, 0x123456789abcdef0).unwrap();
    let grown = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "HeapReAlloc",
        &[heap, 8, ptr, 10000],
    );
    assert_eq!(mem.read_int(grown + 4094, 8).unwrap(), 0x123456789abcdef0);
    assert_eq!(mem.read_int(grown + 9000, 8).unwrap(), 0);
    assert!(mem.read_int(ptr, 1).is_err());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "HeapSize",
            &[heap, 0, grown]
        ),
        10000
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "HeapFree",
            &[heap, 0, grown]
        ),
        1
    );
    assert!(mem.read_int(grown, 1).is_err());
    // No exception-handler reserve exists: query zero, reject a nonzero guarantee.
    mem.write_int(output, 4, 0).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetThreadStackGuarantee",
            &[output]
        ),
        1
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 0);
    mem.write_int(output, 4, 65536).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetThreadStackGuarantee",
            &[output]
        ),
        0
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 65536);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        50
    );
}
