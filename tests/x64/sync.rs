//! Existing behavioral tests grouped by sync; expectations remain independent of API catalogs.

use super::*;

#[test]
fn win64_critical_section_ex_ignores_low_flags_and_preserves_recursive_ownership_and_faults() {
    let (mut m, mut cpu, mut r) = startup();
    let section = STACK + 0x100;
    for flags in [1, 0x01000001, 0x00ffffff] {
        assert_eq!(
            startup_call(
                &mut m,
                &mut cpu,
                &mut r,
                "InitializeCriticalSectionEx",
                &[section, 0xabcdef01000001f4, 0xabcdef0000000000 | flags]
            ),
            1
        );
        assert_eq!(m.read_int(section, 8).unwrap(), u64::MAX);
        assert_eq!(m.read_int(section + 8, 4).unwrap(), u32::MAX as u64);
        assert_eq!(m.read_int(section + 32, 8).unwrap(), 500);
        assert_eq!(
            startup_call(
                &mut m,
                &mut cpu,
                &mut r,
                "TryEnterCriticalSection",
                &[section]
            ),
            1
        );
        startup_call(&mut m, &mut cpu, &mut r, "EnterCriticalSection", &[section]);
        assert_eq!(m.read_int(section + 12, 4).unwrap(), 2);
        let tid = startup_call(&mut m, &mut cpu, &mut r, "GetCurrentThreadId", &[]);
        assert_eq!(m.read_int(section + 16, 8).unwrap(), tid);
        startup_call(&mut m, &mut cpu, &mut r, "LeaveCriticalSection", &[section]);
        assert_eq!(m.read_int(section + 12, 4).unwrap(), 1);
        assert_eq!(m.read_int(section + 16, 8).unwrap(), tid);
        startup_call(&mut m, &mut cpu, &mut r, "LeaveCriticalSection", &[section]);
        assert_eq!(m.read_int(section + 12, 4).unwrap(), 0);
        assert_eq!(m.read_int(section + 16, 8).unwrap(), 0);
        startup_call(
            &mut m,
            &mut cpu,
            &mut r,
            "DeleteCriticalSection",
            &[section],
        );
    }
    for (out, flags) in [(section, 0x10000000), (STACK + 8192 - 24, 1)] {
        m.write(out, &[0xaa; 24]).unwrap();
        cpu.regs[1] = out;
        cpu.regs[2] = 500;
        cpu.regs[8] = flags;
        let before = cpu.clone();
        let calls = r.api_calls;
        assert!(r
            .dispatch(
                "kernel32.dll",
                "InitializeCriticalSectionEx",
                &mut cpu,
                &mut m
            )
            .is_err());
        assert_eq!(cpu.regs, before.regs);
        assert_eq!(cpu.rip, before.rip);
        assert_eq!(r.api_calls, calls);
        let mut bytes = [0; 24];
        m.read(out, &mut bytes).unwrap();
        assert_eq!(bytes, [0xaa; 24]);
    }
}

#[test]
fn win64_waitable_timers_reset_consume_cancel_and_share_named_object_identity() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let data = STACK + 0x200;
    mem.write(data, &[0; 8]).unwrap();
    mem.write(
        data + 16,
        &"Timer\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for manual in [0, 1] {
        let h = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateWaitableTimerExW",
            &[0, 0, manual, 0x1f0003],
        );
        assert_ne!(h, 0);
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "WaitForSingleObject",
                &[h, 0]
            ),
            258
        );
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "SetWaitableTimer",
                &[h, data, 0, 0, 0, 0]
            ),
            1
        );
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "WaitForSingleObject",
                &[h, 0]
            ),
            0
        );
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "CancelWaitableTimer",
                &[h]
            ),
            1
        );
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "WaitForSingleObject",
                &[h, 0]
            ),
            if manual != 0 { 0 } else { 258 }
        );
        mem.write_int(data, 8, (-100_000_000i64) as u64).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "SetWaitableTimer",
                &[h, data, 0, 0, 0, 0]
            ),
            1
        );
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "WaitForSingleObject",
                &[h, 0]
            ),
            258
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[h]),
            1
        );
        mem.write_int(data, 8, 0).unwrap();
    }
    let h = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateWaitableTimerW",
        &[0, 0, data + 16],
    );
    let alias = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "OpenWaitableTimerW",
        &[0x100000, 0, data + 16],
    );
    assert_ne!(alias, 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetWaitableTimer",
            &[alias, data, 0, 0, 0, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        5
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetWaitableTimer",
            &[h, data, 0, 0, 0, 0]
        ),
        1
    );
    mem.write_int(data + 64, 8, h).unwrap();
    mem.write_int(data + 72, 8, alias).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForMultipleObjects",
            &[2, data + 64, 1, 0]
        ),
        u32::MAX as u64
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        87
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[alias, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[h, 0]
        ),
        258
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateEventW",
            &[0, 0, 0, data + 16]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        6
    );
}

#[test]
fn amd64_suspended_thread_context_reads_saved_guest_registers_across_small_and_large_budgets() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x590000000;
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
        mov(code, 0, p.runtime.register("kernel32.dll", name));
        code.extend([0xff, 0xd0]);
    }
    let path =
        std::env::temp_dir().join(format!("wire-thread-context64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    for slice in [1, 100_000] {
        let mut p = Process64::load(&path, false).unwrap();
        p.memory
            .map(CODE, 3 * 4096, READ | WRITE | EXECUTE)
            .unwrap();
        let parameter = 0x8877665544332211;
        let magic = 0x123456789abcdef0;
        for offset in [0, 1536] {
            p.memory.write(DATA + offset, &[0xaa; 1232]).unwrap();
            p.memory.write_int(DATA + offset + 48, 4, 0x100003).unwrap();
        }
        let mut main = vec![0x48, 0x83, 0xec, 0x38];
        call(
            &mut p,
            &mut main,
            "CreateThread",
            &[0, 0, WORKER, parameter, 4, 0],
        );
        main.extend([0x49, 0x89, 0xc4, 0x4c, 0x89, 0xe1]); // r12=handle, rcx=r12
        mov(&mut main, 2, DATA);
        call(&mut p, &mut main, "GetThreadContext", &[]);
        mov(&mut main, 7, DATA + 3072);
        main.extend([0x48, 0x89, 0x07, 0x4c, 0x89, 0xe1]);
        call(&mut p, &mut main, "ResumeThread", &[]);
        call(&mut p, &mut main, "SwitchToThread", &[]);
        main.extend([0x4c, 0x89, 0xe1]);
        call(&mut p, &mut main, "SuspendThread", &[]);
        main.extend([0x4c, 0x89, 0xe1]);
        mov(&mut main, 2, DATA + 1536);
        call(&mut p, &mut main, "GetThreadContext", &[]);
        main.extend([0x48, 0x83, 0xc4, 0x38, 0xc3]);
        p.memory.write(CODE, &main).unwrap();
        let mut worker = Vec::new();
        mov(&mut worker, 13, magic);
        worker.extend([0xeb, 0xfe]);
        p.memory.write(WORKER, &worker).unwrap();
        p.cpu.rip = CODE;
        loop {
            match p.run(p.cpu.work + slice).unwrap() {
                Outcome::Budget => assert!(p.cpu.work < 20000),
                result => {
                    assert_eq!(result, Outcome::Returned(1));
                    break;
                }
            }
        }
        assert_eq!(p.memory.read_int(DATA + 3072, 8).unwrap(), 1);
        assert_eq!(p.memory.read_int(DATA + 128, 8).unwrap(), parameter);
        assert_eq!(p.memory.read_int(DATA + 248, 8).unwrap(), WORKER);
        assert_eq!(p.memory.read_int(DATA + 1536 + 224, 8).unwrap(), magic); // r13
        assert_eq!(
            p.memory.read_int(DATA + 1536 + 248, 8).unwrap(),
            WORKER + 10
        );
        assert_eq!(
            p.memory.read_int(DATA + 1536 + 256, 8).unwrap(),
            0xaaaaaaaaaaaaaaaa
        );
        assert_eq!(
            p.memory.read_int(DATA + 1536, 8).unwrap(),
            0xaaaaaaaaaaaaaaaa
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn amd64_user_apc_wakes_alertable_target_and_preserves_64_bit_parameter_and_return() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x5a0000000;
    const WORKER: u64 = CODE + 4096;
    const CALLBACK: u64 = WORKER + 256;
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
        mov(code, 0, p.runtime.register("kernel32.dll", name));
        code.extend([0xff, 0xd0]);
    }
    let path = std::env::temp_dir().join(format!("wire-apc64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    p.memory
        .map(CODE, 3 * 4096, READ | WRITE | EXECUTE)
        .unwrap();
    let mut main = vec![0x48, 0x83, 0xec, 0x38];
    call(&mut p, &mut main, "SetErrorMode", &[1]);
    call(&mut p, &mut main, "CreateThread", &[0, 0, WORKER, 0, 4, 0]);
    main.extend([0x49, 0x89, 0xc4, 0x4c, 0x89, 0xe1]);
    call(&mut p, &mut main, "ResumeThread", &[]);
    call(&mut p, &mut main, "SwitchToThread", &[]);
    mov(&mut main, 1, CALLBACK);
    main.extend([0x4c, 0x89, 0xe2]); // rdx=r12 target thread
    mov(&mut main, 8, 0x8877665544332211);
    call(&mut p, &mut main, "QueueUserAPC", &[]);
    main.extend([0x4c, 0x89, 0xe1]);
    mov(&mut main, 2, u32::MAX as u64);
    call(&mut p, &mut main, "WaitForSingleObject", &[]);
    call(&mut p, &mut main, "GetThreadErrorMode", &[]);
    main.extend([0x48, 0x83, 0xc4, 0x38, 0xc3]);
    p.memory.write(CODE, &main).unwrap();
    let mut worker = vec![0x48, 0x83, 0xec, 0x28];
    call(&mut p, &mut worker, "GetThreadErrorMode", &[]);
    mov(&mut worker, 7, DATA + 24);
    worker.extend([0x48, 0x89, 0x07]);
    call(&mut p, &mut worker, "SetThreadErrorMode", &[0x8000, 0]);
    call(&mut p, &mut worker, "SleepEx", &[u32::MAX as u64, 1]);
    mov(&mut worker, 7, DATA + 16);
    worker.extend([0x48, 0x89, 0x07, 0x48, 0x83, 0xc4, 0x28, 0xc3]);
    p.memory.write(WORKER, &worker).unwrap();
    let mut callback = vec![0x48, 0x83, 0xec, 0x28];
    mov(&mut callback, 0, DATA);
    callback.extend([0x48, 0x89, 0x08]);
    call(&mut p, &mut callback, "GetCurrentThreadId", &[]);
    mov(&mut callback, 7, DATA + 8);
    callback.extend([0x48, 0x89, 0x07]);
    call(&mut p, &mut callback, "GetThreadErrorMode", &[]);
    mov(&mut callback, 7, DATA + 32);
    callback.extend([
        0x48, 0x89, 0x07, 0xb8, 99, 0, 0, 0, 0x48, 0x83, 0xc4, 0x28, 0xc3,
    ]);
    p.memory.write(CALLBACK, &callback).unwrap();
    p.cpu.rip = CODE;
    loop {
        match p.run(p.cpu.work + 1).unwrap() {
            Outcome::Budget => assert!(p.cpu.work < 1000),
            result => {
                assert_eq!(result, Outcome::Returned(0));
                break;
            }
        }
    }
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), 0x8877665544332211);
    assert_eq!(p.memory.read_int(DATA + 8, 8).unwrap(), 2);
    assert_eq!(p.memory.read_int(DATA + 16, 8).unwrap(), 0xc0);
    assert_eq!(p.memory.read_int(DATA + 24, 8).unwrap(), 1);
    assert_eq!(p.memory.read_int(DATA + 32, 8).unwrap(), 0x8000);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn win64_address_waits_check_size_values_timeouts_and_faults_before_blocking() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let address = STACK + 0x300;
    let compare = address + 16;
    runtime.enable_scheduler();
    mem.write_int(address, 8, 0xff00000000000000).unwrap();
    for size in [1, 2, 4, 8] {
        let result = startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "api-ms-win-core-synch-l1-2-0.dll",
            "WaitOnAddress",
            &[address, compare, size, 0],
        );
        assert_eq!(result, if size == 8 { 1 } else { 0 });
        if size != 8 {
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
                1460
            );
        }
    }
    for size in [0, 3, 16, 0x100000008] {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "WaitOnAddress",
                &[0, 0, size, 0]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            87
        );
    }
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = address;
    cpu.regs[2] = 0x123400000000;
    cpu.regs[8] = 8;
    cpu.regs[9] = 0;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("kernel32.dll", "WaitOnAddress", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    // No readers are required for a wake with no matching waiter.
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "WakeByAddressAll",
        &[0x123400000000],
    );
}

#[test]
fn amd64_address_wait_queues_wake_first_arrival_and_all_across_single_step_budgets() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x580000000;
    const SECOND: u64 = CODE + 4096;
    const FIRST: u64 = CODE + 8192;
    const DATA: u64 = CODE + 12288;
    fn mov(code: &mut Vec<u8>, register: u8, value: u64) {
        code.extend([0x48 | (register >> 3), 0xb8 | (register & 7)]);
        code.extend(value.to_le_bytes());
    }
    fn store(code: &mut Vec<u8>, pointer: u64) {
        mov(code, 7, pointer);
        code.extend([0x48, 0x89, 0x07]);
    }
    fn call(p: &mut Process64, code: &mut Vec<u8>, name: &str, args: &[u64]) {
        for (n, &value) in args.iter().enumerate() {
            mov(code, if n < 4 { [1, 2, 8, 9][n] } else { 0 }, value);
            if n >= 4 {
                code.extend([0x48, 0x89, 0x44, 0x24, 32 + (n as u8 - 4) * 8]);
            }
        }
        let dll = if name == "WaitOnAddress" || name.starts_with("WakeByAddress") {
            "api-ms-win-core-synch-l1-2-0.dll"
        } else if name.starts_with("WSA") {
            "ws2_32.dll"
        } else {
            "kernel32.dll"
        };
        mov(code, 0, p.runtime.register(dll, name));
        code.extend([0xff, 0xd0]);
    }
    let path = std::env::temp_dir().join(format!("wire-address64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    p.memory
        .map(CODE, 4 * 4096, READ | WRITE | EXECUTE)
        .unwrap();
    // Thread 3 waits before thread 2, so a single wake must use arrival order.
    let mut main = vec![0x48, 0x83, 0xec, 0x38];
    call(&mut p, &mut main, "WakeByAddressSingle", &[DATA]); // No wake credit.
    call(&mut p, &mut main, "CreateThread", &[0, 0, SECOND, 0, 4, 0]);
    store(&mut main, DATA + 64);
    call(&mut p, &mut main, "CreateThread", &[0, 0, FIRST, 0, 4, 0]);
    store(&mut main, DATA + 72);
    mov(&mut main, 0, DATA + 72);
    main.extend([0x48, 0x8b, 0x08]);
    call(&mut p, &mut main, "ResumeThread", &[]);
    call(&mut p, &mut main, "SwitchToThread", &[]);
    mov(&mut main, 0, DATA + 64);
    main.extend([0x48, 0x8b, 0x08]);
    call(&mut p, &mut main, "ResumeThread", &[]);
    call(&mut p, &mut main, "SwitchToThread", &[]);
    call(&mut p, &mut main, "WakeByAddressSingle", &[DATA + 128]); // Wrong address.
    call(&mut p, &mut main, "WakeByAddressSingle", &[DATA]);
    call(&mut p, &mut main, "SwitchToThread", &[]);
    mov(&mut main, 0, DATA + 16);
    main.extend([0x48, 0x8b, 0x00]);
    store(&mut main, DATA + 80);
    mov(&mut main, 0, DATA + 24);
    main.extend([0x48, 0x8b, 0x00]);
    store(&mut main, DATA + 88);
    call(&mut p, &mut main, "WakeByAddressAll", &[DATA]);
    mov(&mut main, 0, DATA + 64);
    main.extend([0x48, 0x8b, 0x08]);
    mov(&mut main, 2, u32::MAX as u64);
    call(&mut p, &mut main, "WaitForSingleObject", &[]);
    // A finite address wait must expire even if the earlier wake had no waiter.
    call(
        &mut p,
        &mut main,
        "WaitOnAddress",
        &[DATA + 128, DATA + 8, 8, 1],
    );
    store(&mut main, DATA + 96);
    call(&mut p, &mut main, "GetLastError", &[]);
    store(&mut main, DATA + 104);
    main.extend([0xb8, 1, 0, 0, 0, 0x48, 0x83, 0xc4, 0x38, 0xc3]);
    p.memory.write(CODE, &main).unwrap();
    for (entry, result, last_error, error) in [
        (FIRST, DATA + 16, DATA + 32, 123u64),
        (SECOND, DATA + 24, DATA + 40, 456),
    ] {
        let mut worker = vec![0x48, 0x83, 0xec, 0x28];
        call(&mut p, &mut worker, "WSASetLastError", &[error]);
        call(
            &mut p,
            &mut worker,
            "WaitOnAddress",
            &[DATA, DATA + 8, 8, u32::MAX as u64],
        );
        store(&mut worker, result);
        call(&mut p, &mut worker, "WSAGetLastError", &[]);
        store(&mut worker, last_error);
        worker.extend([0x48, 0x83, 0xc4, 0x28, 0xc3]);
        p.memory.write(entry, &worker).unwrap();
    }
    p.cpu.rip = CODE;
    loop {
        match p.run(p.cpu.work + 1).unwrap() {
            Outcome::Budget => assert!(p.cpu.work < 10000),
            result => {
                assert_eq!(result, Outcome::Returned(1));
                break;
            }
        }
    }
    for (offset, expected) in [
        (16, 1),
        (24, 1),
        (32, 123),
        (40, 456),
        (80, 1),
        (88, 0),
        (96, 0),
        (104, 1460),
    ] {
        assert_eq!(
            p.memory.read_int(DATA + offset, 8).unwrap(),
            expected,
            "offset={offset}"
        );
    }
    assert_eq!(p.memory.read_int(DATA, 8).unwrap(), 0); // Wake can return without value change.
    std::fs::remove_file(path).unwrap();
}

#[test]
fn win64_thread_handle_duplicates_keep_identity_and_independent_flags_without_closing_thread() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let out = STACK + 0x100;
    let flags = STACK + 0x180;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, u64::MAX - 1, u64::MAX, out, 0, 1, 2]
        ),
        1
    );
    let first = mem.read_int(out, 8).unwrap();
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetThreadId", &[first]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[first, flags]
        ),
        1
    );
    assert_eq!(mem.read_int(flags, 4).unwrap(), 1);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, first, u64::MAX, out, 0, 0, 2]
        ),
        1
    );
    let second = mem.read_int(out, 8).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetHandleInformation",
            &[first, 2, 2]
        ),
        1
    );
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
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[first]),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetHandleInformation",
            &[first, 2, 0]
        ),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[first]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetThreadId", &[second]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetExitCodeThread",
            &[second, flags]
        ),
        1
    );
    assert_eq!(mem.read_int(flags, 4).unwrap(), 259);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, second, u64::MAX, out, 0, 1, 3]
        ),
        1
    );
    let third = mem.read_int(out, 8).unwrap();
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetThreadId", &[second]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetThreadId", &[third]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[third, flags]
        ),
        1
    );
    assert_eq!(mem.read_int(flags, 4).unwrap(), 1);
    mem.write_int(out, 8, 0xfeed).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[u64::MAX, third, u64::MAX, out, 0x40, 0, 0]
        ),
        0
    );
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xfeed);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        50
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[third]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetCurrentThreadId", &[]),
        1
    );
}

#[test]
fn win64_events_share_named_state_consume_auto_reset_and_validate_wait_access() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let name = STACK + 0x100;
    mem.write(
        name,
        &"Local\\wire-event\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let auto = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateEventW",
        &[0, 0, 1, name],
    );
    let alias = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateEventW",
        &[0, 1, 0, name],
    );
    assert_ne!(auto, alias);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        183
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[alias, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[auto, 0]
        ),
        258
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "SetEvent", &[auto]),
        1
    );
    let manual = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateEventW",
        &[0, 1, 0, 0],
    );
    let array = STACK + 0x200;
    mem.write_int(array, 8, auto).unwrap();
    mem.write_int(array + 8, 8, manual).unwrap();
    // A failed wait-all must not consume the already signaled auto-reset event.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForMultipleObjects",
            &[2, array, 1, 0]
        ),
        258
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "SetEvent", &[manual]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForMultipleObjects",
            &[2, array, 1, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[auto, 0]
        ),
        258
    );
    for _ in 0..2 {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "WaitForSingleObject",
                &[manual, 0]
            ),
            0
        );
    }
    let read = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "OpenEventW",
        &[0x100000, 0, name],
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "SetEvent", &[read]),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[auto]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "SetEvent", &[alias]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[read, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[read]),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "OpenEventW",
            &[0x100000, 0, name]
        ),
        0
    );
    cpu.regs[1] = manual;
    cpu.regs[2] = u32::MAX as u64;
    startup_call(&mut mem, &mut cpu, &mut runtime, "ResetEvent", &[manual]);
    cpu.regs[1] = manual;
    cpu.regs[2] = u32::MAX as u64;
    mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "WaitForSingleObject", &mut cpu, &mut mem)
        .unwrap_err()
        .contains("scheduler"));
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
}

#[test]
fn win64_semaphores_count_wait_atomically_and_share_event_namespace() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let name = STACK + 0x100;
    let output = STACK + 0x200;
    let array = STACK + 0x300;
    mem.write(
        name,
        &"wire-semaphore\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let sem = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateSemaphoreExW",
        &[0, 1, 2, name, 0, 0x1f0003],
    );
    assert_ne!(sem, 0);
    let alias = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateSemaphoreW",
        &[0, 0, 1, name],
    );
    assert_ne!(alias, 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        183
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateEventW",
            &[0, 0, 0, name]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        6
    );
    let event = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateEventW",
        &[0, 0, 0, 0],
    );
    mem.write_int(array, 8, sem).unwrap();
    mem.write_int(array + 8, 8, event).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForMultipleObjects",
            &[2, array, 1, 0]
        ),
        258
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetEvent", &[event]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForMultipleObjects",
            &[2, array, 1, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[alias, 0]
        ),
        258
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReleaseSemaphore",
            &[alias, 2, output]
        ),
        1
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 0);
    mem.write_int(output, 4, 0xfeed).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReleaseSemaphore",
            &[alias, 1, output]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        298
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 0xfeed);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReleaseSemaphore",
            &[alias, u32::MAX as u64, output]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        87
    );
    let read = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "OpenSemaphoreW",
        &[0x100000, 0, name],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReleaseSemaphore",
            &[read, 1, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[read, 0]
        ),
        0
    );
    // Bad previous-count pointer must not increment the live object.
    cpu.regs[1] = sem;
    cpu.regs[2] = 1;
    cpu.regs[8] = 2;
    mem.write_int(cpu.regs[4], 8, BASE + 0x100).unwrap();
    let before = cpu.clone();
    assert!(runtime
        .dispatch("kernel32.dll", "ReleaseSemaphore", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[sem, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "WaitForSingleObject",
            &[sem, 0]
        ),
        258
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[sem]);
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "ReleaseSemaphore",
            &[read, 1, 0]
        ),
        0
    );
    startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[read]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "OpenSemaphoreW",
            &[0x100000, 0, name]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CreateSemaphoreW",
            &[0, 3, 2, 0]
        ),
        0
    );
}

#[test]
fn amd64_guest_threads_run_callbacks_and_isolate_tls_fls_teb_and_mxcsr() {
    use wire::x64::process::{Outcome, Process64};
    const CODE: u64 = 0x500000000;
    const WORKER: u64 = CODE + 4096;
    const DATA: u64 = CODE + 8192;
    const DESTRUCTOR: u64 = CODE + 12288;
    fn mov(code: &mut Vec<u8>, register: u8, value: u64) {
        code.extend([0x48 | (register >> 3), 0xb8 | (register & 7)]);
        code.extend(value.to_le_bytes());
    }
    fn store(code: &mut Vec<u8>, address: u64) {
        mov(code, 7, address);
        code.extend([0x48, 0x89, 0x07]);
    }
    fn call(p: &mut Process64, code: &mut Vec<u8>, name: &str, args: &[(u64, bool)]) {
        for (n, &(value, indirect)) in args.iter().enumerate() {
            let reg = if n < 4 { [1, 2, 8, 9][n] } else { 0 };
            mov(code, reg, value);
            if indirect {
                // Load through RAX without disturbing the other argument registers.
                mov(code, 0, value);
                code.extend([0x48, 0x8b, 0x00]);
                if reg != 0 {
                    code.extend([0x48 | (reg >> 3), 0x89, 0xc0 | (reg & 7)]);
                }
            }
            if n >= 4 {
                code.extend([0x48, 0x89, 0x44, 0x24, 32 + (n as u8 - 4) * 8]);
            }
        }
        mov(code, 0, p.runtime.register("kernel32.dll", name));
        code.extend([0xff, 0xd0]);
    }
    let mut b = fixture();
    put32(&mut b, 0x98 + 112 + 9 * 8, 0x1100);
    put32(&mut b, 0x98 + 116 + 9 * 8, 40);
    put64(&mut b, 0x300, BASE + 0x11c0);
    put64(&mut b, 0x308, BASE + 0x11c4);
    put64(&mut b, 0x310, BASE + 0x1180);
    put64(&mut b, 0x318, BASE + 0x1190);
    put64(&mut b, 0x390, BASE + 0x1220);
    put32(&mut b, 0x3c0, 0x1234);
    let mut callback = vec![
        0x65, 0x48, 0x8b, 0x04, 0x25, 0x58, 0, 0, 0, 0x48, 0x8b, 0x00, 0xff, 0x00,
    ];
    mov(&mut callback, 0, DATA + 112);
    callback.extend([0xff, 0x00, 0xc3]);
    b[0x420..0x420 + callback.len()].copy_from_slice(&callback);
    let mut jump = Vec::new();
    mov(&mut jump, 0, CODE);
    jump.extend([0xff, 0xe0]);
    b[0x200..0x200 + jump.len()].copy_from_slice(&jump);
    let path = std::env::temp_dir().join(format!("wire-thread64-{}.exe", std::process::id()));
    std::fs::write(&path, b).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    p.memory
        .map(CODE, 4 * 4096, READ | WRITE | EXECUTE)
        .unwrap();
    let mut main = vec![0x48, 0x83, 0xec, 0x38];
    call(
        &mut p,
        &mut main,
        "CreateSemaphoreW",
        &[(0, false), (0, false), (1, false), (0, false)],
    );
    store(&mut main, DATA);
    call(&mut p, &mut main, "TlsAlloc", &[]);
    assert_eq!(p.memory.read_int(BASE + 0x1180, 4).unwrap(), 0);
    call(
        &mut p,
        &mut main,
        "TlsSetValue",
        &[(0, false), (0x99, false)],
    );
    call(&mut p, &mut main, "FlsAlloc", &[(DESTRUCTOR, false)]);
    call(
        &mut p,
        &mut main,
        "FlsSetValue",
        &[(0, false), (0x77, false)],
    );
    main.extend([
        0x65, 0x48, 0x8b, 0x04, 0x25, 0x58, 0, 0, 0, 0x48, 0x8b, 0x00, 0xc7, 0x00, 0x11, 0x11, 0, 0,
    ]);
    mov(&mut main, 7, DATA + 120);
    main.extend([0x0f, 0xae, 0x17]); // LDMXCSR [RDI]
    mov(&mut main, 7, DATA + 160);
    main.extend([0xd9, 0x2f]);
    call(&mut p, &mut main, "SetLastError", &[(123, false)]);
    call(
        &mut p,
        &mut main,
        "CreateThread",
        &[
            (0, false),
            (0, false),
            (WORKER, false),
            (DATA, false),
            (4, false),
            (DATA + 24, false),
        ],
    );
    store(&mut main, DATA + 16);
    call(
        &mut p,
        &mut main,
        "GetExitCodeThread",
        &[(DATA + 16, true), (DATA + 32, false)],
    );
    call(
        &mut p,
        &mut main,
        "WaitForSingleObject",
        &[(DATA + 16, true), (0, false)],
    );
    store(&mut main, DATA + 40);
    call(&mut p, &mut main, "ResumeThread", &[(DATA + 16, true)]);
    store(&mut main, DATA + 48);
    call(
        &mut p,
        &mut main,
        "WaitForSingleObject",
        &[(DATA, true), (u32::MAX as u64, false)],
    );
    call(
        &mut p,
        &mut main,
        "WaitForSingleObject",
        &[(DATA + 16, true), (u32::MAX as u64, false)],
    );
    call(
        &mut p,
        &mut main,
        "GetExitCodeThread",
        &[(DATA + 16, true), (DATA + 56, false)],
    );
    call(&mut p, &mut main, "GetLastError", &[]);
    store(&mut main, DATA + 64);
    call(&mut p, &mut main, "TlsGetValue", &[(0, false)]);
    store(&mut main, DATA + 72);
    call(&mut p, &mut main, "FlsGetValue", &[(0, false)]);
    store(&mut main, DATA + 80);
    main.extend([0x48, 0x83, 0xc4, 0x38, 0xb8, 1, 0, 0, 0, 0xc3]);
    let mut worker = vec![0x48, 0x83, 0xec, 0x38, 0x48, 0x89, 0xc8];
    store(&mut worker, DATA + 88); // Exact lpParameter.
    mov(&mut worker, 7, DATA + 164);
    worker.extend([0xd9, 0x3f]);
    mov(&mut worker, 7, DATA + 168);
    worker.extend([0xd9, 0x2f]);
    worker.extend([0x65, 0x48, 0x8b, 0x04, 0x25, 0x30, 0, 0, 0]);
    store(&mut worker, DATA + 96);
    worker.extend([
        0x65, 0x48, 0x8b, 0x04, 0x25, 0x58, 0, 0, 0, 0x48, 0x8b, 0x00, 0x8b, 0x00,
    ]);
    store(&mut worker, DATA + 104);
    call(&mut p, &mut worker, "TlsGetValue", &[(0, false)]);
    store(&mut worker, DATA + 128);
    call(&mut p, &mut worker, "FlsGetValue", &[(0, false)]);
    store(&mut worker, DATA + 136);
    call(&mut p, &mut worker, "SetLastError", &[(42, false)]);
    call(
        &mut p,
        &mut worker,
        "TlsSetValue",
        &[(0, false), (0x11, false)],
    );
    call(
        &mut p,
        &mut worker,
        "FlsSetValue",
        &[(0, false), (0x88, false)],
    );
    mov(&mut worker, 7, DATA + 124);
    worker.extend([0x0f, 0xae, 0x17]);
    call(
        &mut p,
        &mut worker,
        "ReleaseSemaphore",
        &[(DATA, true), (1, false), (0, false)],
    );
    worker.extend([0x48, 0x83, 0xc4, 0x38, 0xb8, 77, 0, 0, 0, 0xc3]);
    let mut destructor = vec![0x48, 0x89, 0xc8];
    store(&mut destructor, DATA + 144);
    mov(&mut destructor, 7, DATA + 172);
    destructor.extend([0xd9, 0x3f]);
    destructor.push(0xc3);
    p.memory.write(CODE, &main).unwrap();
    p.memory.write(WORKER, &worker).unwrap();
    p.memory.write(DESTRUCTOR, &destructor).unwrap();
    p.memory.write_int(DATA + 120, 4, 0x3f80).unwrap();
    p.memory.write_int(DATA + 124, 4, 0x5f80).unwrap();
    p.memory.write_int(DATA + 160, 2, 0x0b7f).unwrap();
    p.memory.write_int(DATA + 168, 2, 0x077f).unwrap();
    assert_eq!(p.run(10000).unwrap(), Outcome::Returned(1));
    assert_eq!(p.memory.read_int(DATA + 24, 4).unwrap(), 2);
    assert_eq!(p.memory.read_int(DATA + 32, 4).unwrap(), 259);
    assert_eq!(p.memory.read_int(DATA + 40, 8).unwrap(), 258);
    assert_eq!(p.memory.read_int(DATA + 48, 8).unwrap(), 1);
    assert_eq!(p.memory.read_int(DATA + 56, 4).unwrap(), 77);
    assert_eq!(p.memory.read_int(DATA + 64, 8).unwrap(), 123);
    assert_eq!(p.memory.read_int(DATA + 72, 8).unwrap(), 0x99);
    assert_eq!(p.memory.read_int(DATA + 80, 8).unwrap(), 0x77);
    assert_eq!(p.memory.read_int(DATA + 88, 8).unwrap(), DATA);
    assert_ne!(
        p.memory.read_int(DATA + 96, 8).unwrap(),
        wire::x64::runtime::TEB
    );
    assert!(p
        .memory
        .read_int(p.memory.read_int(DATA + 96, 8).unwrap(), 1)
        .is_err()); // Exited TEB released.
    assert_eq!(p.memory.read_int(DATA + 104, 8).unwrap(), 0x1235); // Fresh template + thread attach.
    assert_eq!(p.memory.read_int(DATA + 112, 4).unwrap(), 3); // Process attach, thread attach/detach.
    assert_eq!(p.memory.read_int(DATA + 128, 8).unwrap(), 0);
    assert_eq!(p.memory.read_int(DATA + 136, 8).unwrap(), 0);
    assert_eq!(p.memory.read_int(DATA + 144, 8).unwrap(), 0x88);
    assert_eq!(p.cpu.mxcsr, 0x3f80);
    assert_eq!(p.cpu.fpu.cw, 0x0b7f);
    assert_eq!(p.memory.read_int(DATA + 164, 2).unwrap(), 0x037f);
    assert_eq!(p.memory.read_int(DATA + 172, 2).unwrap(), 0x077f);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn amd64_timed_sleep_survives_budget_pause_and_wait_timeout_uses_elapsed_time() {
    use std::time::{Duration, Instant};
    use wire::x64::process::{Outcome, Process64};
    let path = std::env::temp_dir().join(format!("wire-sleep64-{}.exe", std::process::id()));
    std::fs::write(&path, fixture()).unwrap();
    let mut p = Process64::load(&path, false).unwrap();
    let sleep = p.runtime.register("kernel32.dll", "Sleep");
    let mut code = vec![0x48, 0x83, 0xec, 0x28, 0xb9, 40, 0, 0, 0, 0x48, 0xb8];
    code.extend(sleep.to_le_bytes());
    code.extend([0xff, 0xd0, 0x48, 0x83, 0xc4, 0x28, 0xb8, 1, 0, 0, 0, 0xc3]);
    p.memory.write(BASE + 0x1000, &code).unwrap();
    let start = Instant::now();
    assert_eq!(p.run(5).unwrap(), Outcome::Budget);
    assert_eq!(p.cpu.retired, 4);
    assert_eq!(p.cpu.work, 5);
    assert_eq!(p.run(1000).unwrap(), Outcome::Returned(1));
    assert!(start.elapsed() >= Duration::from_millis(40));
    assert_eq!(p.cpu.retired, 7);
    let (mut mem, mut cpu, mut runtime) = startup();
    runtime.enable_scheduler();
    let event = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "CreateEventW",
        &[0, 0, 0, 0],
    );
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    cpu.regs[1] = event;
    cpu.regs[2] = 10;
    let before = cpu.clone();
    assert_eq!(
        runtime
            .dispatch("kernel32.dll", "WaitForSingleObject", &mut cpu, &mut mem)
            .unwrap(),
        wire::x64::runtime::Control::Blocked
    );
    assert_eq!(cpu.regs, before.regs);
    assert_eq!(cpu.rip, before.rip);
    std::thread::sleep(Duration::from_millis(15));
    assert_eq!(
        runtime
            .dispatch("kernel32.dll", "WaitForSingleObject", &mut cpu, &mut mem)
            .unwrap(),
        wire::x64::runtime::Control::Returned
    );
    assert_eq!(cpu.regs[0], 258);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn win64_srw_ownership_and_init_once_publish_context_without_fake_contention() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let lock = STACK + 0x100;
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "AcquireSRWLockShared",
        &[lock],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TryAcquireSRWLockExclusive",
            &[lock]
        ),
        0
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "ReleaseSRWLockShared",
        &[lock],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "TryAcquireSRWLockExclusive",
            &[lock]
        ),
        1
    );
    startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "ReleaseSRWLockExclusive",
        &[lock],
    );
    assert_eq!(mem.read_int(lock, 8).unwrap(), 0);
    let pending = STACK + 0x200;
    let context = STACK + 0x300;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "InitOnceBeginInitialize",
            &[lock, 0, pending, context]
        ),
        1
    );
    assert_eq!(mem.read_int(pending, 4).unwrap(), 1);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "InitOnceComplete",
            &[lock, 0, 0x123456780000]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "InitOnceBeginInitialize",
            &[lock, 1, pending, context]
        ),
        1
    );
    assert_eq!(mem.read_int(pending, 4).unwrap(), 0);
    assert_eq!(mem.read_int(context, 8).unwrap(), 0x123456780000);
}
