//! Independent thread lifecycle, control-outcome and description baseline.
use super::*;
use wire::x64::runtime::Control;

fn request(
    mem: &mut Memory,
    cpu: &mut Cpu64,
    runtime: &mut wire::x64::runtime::Runtime64,
    name: &str,
    args: &[u64],
) -> Result<Control, String> {
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
    runtime.dispatch("kernelbase.dll", name, cpu, mem)
}

#[test]
fn thread_control_keeps_win64_description_identity_hresult_and_local_ownership() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    let output = STACK + 0x200;
    let thread = u64::MAX - 1;
    let data: Vec<_> = "线程🎮"
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    mem.write(text, &data).unwrap();
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                dll,
                "SetThreadDescription",
                &[thread, text]
            ),
            0
        );
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                dll,
                "GetThreadDescription",
                &[thread, output]
            ),
            0
        );
        let pointer = mem.read_int(output, 8).unwrap();
        let mut readback = vec![0; data.len()];
        mem.read(pointer, &mut readback).unwrap();
        assert_eq!(readback, data);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "LocalSize", &[pointer]),
            data.len() as u64
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "LocalFree", &[pointer]),
            0
        );
        for name in ["GetThreadDescription", "SetThreadDescription"] {
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    dll,
                    name,
                    &[0xfffffffe, 0xdead0000]
                ),
                0x80070006
            );
        }
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            123
        );
    }
    // Output overwrites the saved return address, but the CPU uses the old frame.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetThreadDescription",
            &[thread, STACK + 0x1f88]
        ),
        0
    );
    let pointer = mem.read_int(STACK + 0x1f88, 8).unwrap();
    assert_ne!(pointer, BASE);
    assert_eq!(cpu.rip, BASE);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "LocalFree", &[pointer]),
        0
    );
    let before = rt.api_calls;
    assert!(request(
        &mut mem,
        &mut cpu,
        &mut rt,
        "SetThreadDescription",
        &[thread, 0xdead0000]
    )
    .is_err());
    assert_eq!(rt.api_calls, before);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetThreadDescription",
            &[thread, output]
        ),
        0
    );
    let pointer = mem.read_int(output, 8).unwrap();
    let mut readback = vec![0; data.len()];
    mem.read(pointer, &mut readback).unwrap();
    assert_eq!(readback, data);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "LocalFree", &[pointer]),
        0
    );
    mem.virtual_protect(STACK, 4096, 4 | wire::mem::PAGE_GUARD)
        .unwrap();
    let calls = rt.api_calls;
    assert!(request(
        &mut mem,
        &mut cpu,
        &mut rt,
        "GetThreadDescription",
        &[thread, output]
    )
    .unwrap_err()
    .contains("guard-page"));
    assert_eq!(rt.api_calls, calls);
    // A failed copy-out preflight must not consume a heap allocation.
    let next = startup_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "GetThreadDescription",
        &[thread, output],
    );
    assert_eq!(next, 0);
    let next_pointer = mem.read_int(output, 8).unwrap();
    assert_eq!(next_pointer, pointer + 4096);
}

#[test]
fn thread_control_keeps_win64_creation_validation_and_mapping_rollback() {
    let (mut mem, mut cpu, mut rt) = startup();
    assert!(request(
        &mut mem,
        &mut cpu,
        &mut rt,
        "CreateThread",
        &[0xdead0000, 0, BASE, 0, 8, 0]
    )
    .unwrap_err()
    .contains("scheduler"));
    rt.enable_scheduler();
    let tid = STACK + 0x100;
    mem.write_int(tid, 4, 0xabcdef12).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "CreateThread",
            &[0xdead0000, 0, BASE, 0, 8, tid]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    assert_eq!(mem.read_int(tid, 4).unwrap(), 0xabcdef12);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "CreateThread",
            &[0, 0x1_00000000, BASE, 0, 4, tid]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        8
    );
    assert!(request(
        &mut mem,
        &mut cpu,
        &mut rt,
        "CreateThread",
        &[0, 0, BASE, 0, 4, 0xdead0000]
    )
    .is_err());
    // Pre-existing mappings survive rollback; failed attempts do not consume IDs.
    let stack = 0x7fe000000000 + 2 * 64 * 1024 * 1024;
    mem.map(stack, 65536, READ | WRITE).unwrap();
    mem.write_int(stack, 8, 0x12345678).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "CreateThread",
            &[0, 0, BASE, 0, 4, tid]
        ),
        0
    );
    assert_eq!(mem.read_int(stack, 8).unwrap(), 0x12345678);
    mem.unmap(stack, 65536).unwrap();
    let security = STACK + 0x200;
    mem.write_int(security, 4, 24).unwrap();
    mem.write_int(security + 8, 8, 1).unwrap();
    assert!(request(
        &mut mem,
        &mut cpu,
        &mut rt,
        "CreateThread",
        &[security, 0, BASE, 0, 4, tid]
    )
    .unwrap_err()
    .contains("security descriptor"));
    mem.write_int(security + 8, 8, 0).unwrap();
    mem.write_int(security + 16, 4, 1).unwrap();
    let h = startup_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "CreateThread",
        &[
            security,
            1,
            BASE,
            0xabcdef12_34567890,
            0x12340000_00010004,
            tid,
        ],
    );
    assert_eq!(mem.read_int(tid, 4).unwrap(), 2);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetThreadId", &[h]),
        2
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetHandleInformation",
            &[h, tid + 4]
        ),
        1
    );
    assert_eq!(mem.read_int(tid + 4, 4).unwrap(), 1);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "ResumeThread", &[h]),
        1
    );
}

#[test]
fn thread_control_keeps_win64_context_errors_trace_and_control_commit() {
    let (mut mem, mut cpu, mut rt) = startup();
    rt.enable_scheduler();
    let h = startup_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "CreateThread",
        &[0, 0, BASE, 0, 4, 0],
    );
    let context = STACK + 0x100;
    for (handle, out, error) in [
        (0, 0xdead0001, 6),
        (u64::MAX - 1, 0xdead0001, 50),
        (h, context + 1, 998),
    ] {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "GetThreadContext",
                &[handle, out]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            error
        );
    }
    for flags in [0, 0x100020, 0x10001f] {
        mem.write_int(context + 48, 4, flags).unwrap();
        let calls = rt.api_calls;
        let trace = rt.trace.len();
        let outcome = request(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetThreadContext",
            &[h, context],
        )
        .unwrap();
        assert_eq!(rt.api_calls, calls + 1);
        if flags == 0x10001f {
            assert_eq!(
                outcome,
                Control::ThreadContext {
                    dll: "kernelbase.dll".into(),
                    args: vec![h, context],
                    thread: 2,
                    flags: flags as u32
                }
            );
            assert_eq!(cpu.rip, BASE);
            assert_eq!(cpu.regs[0], 1);
            assert_eq!(rt.trace.len(), trace + 1);
        } else {
            assert_eq!(outcome, Control::Returned);
            assert_eq!(cpu.regs[0], 0);
            assert_eq!(rt.trace.len(), trace + 2);
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
                87
            );
        }
    }
    mem.virtual_protect(STACK, 4096, 4 | wire::mem::PAGE_GUARD)
        .unwrap();
    let calls = rt.api_calls;
    let trace = rt.trace.len();
    assert!(request(
        &mut mem,
        &mut cpu,
        &mut rt,
        "GetThreadContext",
        &[h, context]
    )
    .unwrap_err()
    .contains("guard-page"));
    assert_eq!(rt.api_calls, calls);
    assert_eq!(rt.trace.len(), trace);
}

#[test]
fn thread_control_keeps_win64_apc_admission_capacity_and_callback_fault_order() {
    let (mut mem, mut cpu, mut rt) = startup();
    let target = STACK + 0x100;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "QueueUserAPC",
            &[0xdead0000, 0, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        6
    );
    assert!(request(
        &mut mem,
        &mut cpu,
        &mut rt,
        "QueueUserAPC",
        &[target, u64::MAX - 1, 1]
    )
    .is_err());
    mem.protect(STACK, 4096, READ | WRITE | EXECUTE).unwrap();
    for i in 0..256 {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "QueueUserAPC",
                &[target, u64::MAX - 1, 0xabcdef12_00000000 | i]
            ),
            1
        );
    }
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "QueueUserAPC",
            &[0xdead0000, u64::MAX - 1, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        8
    );
    mem.protect(STACK, 4096, READ | WRITE).unwrap();
    let calls = rt.api_calls;
    assert!(request(&mut mem, &mut cpu, &mut rt, "SleepEx", &[0, 1]).is_err());
    assert_eq!(rt.api_calls, calls);
    mem.protect(STACK, 4096, READ | WRITE | EXECUTE).unwrap();
    let Control::Callbacks { calls, .. } =
        request(&mut mem, &mut cpu, &mut rt, "SleepEx", &[0, 1]).unwrap()
    else {
        panic!("expected APC callbacks")
    };
    assert_eq!(calls.len(), 256);
    for (i, &(callback, args)) in calls.iter().enumerate() {
        assert_eq!(callback, target);
        assert_eq!(args, [0xabcdef12_00000000 | i as u64, 0, 0, 0]);
    }
    assert_eq!(cpu.regs[0], 0xc0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "SleepEx", &[0, 1]),
        0
    );
}

#[test]
fn thread_control_keeps_win64_exit_cpu_and_provider_boundaries() {
    let (mut mem, mut cpu, mut rt) = startup();
    assert!(request(&mut mem, &mut cpu, &mut rt, "ExitThread", &[42])
        .unwrap_err()
        .contains("scheduler"));
    rt.enable_scheduler();
    let calls = rt.api_calls;
    let trace = rt.trace.len();
    assert_eq!(
        request(
            &mut mem,
            &mut cpu,
            &mut rt,
            "ExitThread",
            &[0xabcdef12_12345678]
        )
        .unwrap(),
        Control::ThreadExited(0x12345678)
    );
    assert_eq!(cpu.regs[4], STACK + 0x1f88);
    assert_eq!(rt.api_calls, calls + 1);
    assert_eq!(rt.trace.len(), trace + 1);
    cpu.regs[4] = 0xdead0000;
    let before = cpu.clone();
    assert!(rt
        .dispatch("kernel32.dll", "ExitThread", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    for name in [
        "CreateThread",
        "ExitThread",
        "QueueUserAPC",
        "GetThreadContext",
        "SetThreadDescription",
        "GetThreadDescription",
    ] {
        let calls = rt.api_calls;
        assert!(rt
            .dispatch("user32.dll", name, &mut cpu, &mut mem)
            .unwrap_err()
            .contains("unimplemented Win64 import"));
        assert_eq!(rt.api_calls, calls);
        assert_eq!(cpu, before);
    }
}
