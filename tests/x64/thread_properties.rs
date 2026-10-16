//! Thread scalar behavior fixed before moving the handlers into their owner.
use super::*;

#[test]
fn thread_properties_keep_win64_widths_priority_and_counted_suspension() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let thread = u64::MAX - 1;
    let high = 0xabcdef12_00000000;
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "GetThreadId",
                &[thread]
            ),
            1
        );
        for priority in [-15i32, -2, -1, 0, 1, 2, 15] {
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    "SetThreadPriority",
                    &[thread, high | priority as u32 as u64]
                ),
                1
            );
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    "GetThreadPriority",
                    &[thread]
                ),
                priority as u32 as u64
            );
        }
        for (name, args, expected, error) in [
            ("GetThreadId", vec![0xfffffffe], 0, 6),
            ("SetThreadPriority", vec![0, 99], 0, 6),
            ("GetThreadPriority", vec![0], 0x7fffffff, 6),
            ("SetThreadPriority", vec![thread, 3], 0, 87),
            ("SetThreadAffinityMask", vec![thread, high | 1], 0, 87),
            ("SetThreadAffinityMask", vec![0, 0], 0, 6),
            ("SuspendThread", vec![0], u32::MAX as u64, 6),
            ("ResumeThread", vec![0], u32::MAX as u64, 6),
        ] {
            assert_eq!(
                startup_dll_call(&mut mem, &mut cpu, &mut runtime, dll, name, &args),
                expected,
                "{dll}!{name}"
            );
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
                error
            );
        }
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "GetThreadPriority",
                &[thread]
            ),
            15
        );
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "SetThreadAffinityMask",
                &[thread, 1]
            ),
            1
        );
        for count in 0..127 {
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    "SuspendThread",
                    &[thread]
                ),
                count
            );
        }
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "SuspendThread",
                &[thread]
            ),
            u32::MAX as u64
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            87
        );
        for count in (1..=127).rev() {
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    dll,
                    "ResumeThread",
                    &[thread]
                ),
                count
            );
        }
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                dll,
                "ResumeThread",
                &[thread]
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
            87
        );
    }
}

#[test]
fn thread_properties_keep_win64_mode_output_faults_and_saved_return_alias() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let high = 0xabcdef12_00000000;
    let output = STACK + 0x100;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetThreadErrorMode",
            &[high | 0x8007, output]
        ),
        1
    );
    assert_eq!(mem.read_int(output, 4).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetThreadErrorMode",
            &[8, 0xdead0000]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        87
    );
    mem.virtual_protect(STACK, 4096, 4 | wire::mem::PAGE_GUARD)
        .unwrap();
    cpu.regs[4] = STACK + 0x1f88;
    cpu.regs[1] = 1;
    cpu.regs[2] = output;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    let trace = runtime.trace.len();
    assert!(runtime
        .dispatch("kernelbase.dll", "SetThreadErrorMode", &mut cpu, &mut mem)
        .unwrap_err()
        .contains("guard-page"));
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(runtime.trace.len(), trace);
    assert_eq!(mem.read_int(output, 4).unwrap(), 0);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetThreadErrorMode", &[]),
        0x8007
    );
    // The saved return frame survives a DWORD output overlapping its slot.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetThreadErrorMode",
            &[2, STACK + 0x1f88]
        ),
        1
    );
    assert_eq!(mem.read_int(STACK + 0x1f88, 8).unwrap(), 0x100008007);
    assert_eq!(cpu.rip, BASE);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetThreadErrorMode", &[]),
        2
    );
    // Return-frame preflight precedes even a scalar state update.
    cpu.regs[4] = 0xdead0000;
    cpu.regs[1] = u64::MAX - 1;
    cpu.regs[2] = 15;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("kernel32.dll", "SetThreadPriority", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetThreadPriority",
            &[u64::MAX - 1]
        ),
        0
    );
}

#[test]
fn thread_properties_keep_win64_exit_output_identity_and_provider_boundaries() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let output = STACK + 0x100;
    mem.write_int(output, 4, 0x1234).unwrap();
    mem.virtual_protect(STACK, 4096, 4 | wire::mem::PAGE_GUARD)
        .unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetExitCodeThread",
            &[0, output]
        ),
        0
    );
    // Invalid handles return before touching the output, leaving its guard armed.
    cpu.regs[4] = STACK + 0x1f88;
    cpu.regs[1] = u64::MAX - 1;
    cpu.regs[2] = output;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("kernel32.dll", "GetExitCodeThread", &mut cpu, &mut mem)
        .unwrap_err()
        .contains("guard-page"));
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(mem.read_int(output, 4).unwrap(), 0x1234);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetExitCodeThread",
            &[u64::MAX - 1, STACK + 0x1f88]
        ),
        1
    );
    assert_eq!(mem.read_int(STACK + 0x1f88, 8).unwrap(), 0x100000103);
    assert_eq!(cpu.rip, BASE);
    for name in [
        "GetThreadId",
        "GetThreadPriority",
        "SetThreadPriority",
        "GetExitCodeThread",
        "SetThreadAffinityMask",
        "SuspendThread",
        "ResumeThread",
        "GetThreadErrorMode",
        "SetThreadErrorMode",
    ] {
        let before = cpu.clone();
        let calls = runtime.api_calls;
        assert!(runtime
            .dispatch("user32.dll", name, &mut cpu, &mut mem)
            .unwrap_err()
            .contains("unimplemented Win64 import"));
        assert_eq!(cpu, before);
        assert_eq!(runtime.api_calls, calls);
    }
}
