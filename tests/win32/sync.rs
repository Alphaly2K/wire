//! Existing behavioral tests grouped by sync; expectations remain independent of API catalogs.

use super::*;

#[test]
fn address_waits_keep_pe32_unknown_abi_and_zero_argument_cleanup() {
    let mut v = Vm::new();
    for dll in [
        "kernel32.dll",
        "kernelbase.dll",
        "api-ms-win-core-synch-l1-2-0.dll",
    ] {
        for (name, args) in [
            ("WaitOnAddress", vec![DATA, DATA + 8, 8, 0]),
            ("WakeByAddressSingle", vec![DATA]),
            ("WakeByAddressAll", vec![DATA]),
        ] {
            v.mem.write_bytes(DATA, &[0xaa; 16]).unwrap();
            assert_eq!(v.call(dll, name, &args), 0);
            assert_eq!(v.state.last_error, 120);
            assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
            assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xaaaaaaaa);
        }
    }
}

#[test]
fn inline_sync_keeps_pe32_abi_only_stack_cleanup_and_untouched_outputs() {
    let mut v = Vm::new();
    for (name, args) in [
        ("InitializeSRWLock", vec![DATA]),
        ("AcquireSRWLockExclusive", vec![DATA]),
        ("AcquireSRWLockShared", vec![DATA]),
        ("ReleaseSRWLockExclusive", vec![DATA]),
        ("ReleaseSRWLockShared", vec![DATA]),
        ("TryAcquireSRWLockExclusive", vec![DATA]),
        ("TryAcquireSRWLockShared", vec![DATA]),
        ("InitializeConditionVariable", vec![DATA]),
        ("SleepConditionVariableSRW", vec![DATA, DATA + 8, 0, 0]),
        ("WakeConditionVariable", vec![DATA]),
        ("WakeAllConditionVariable", vec![DATA]),
        (
            "InitOnceBeginInitialize",
            vec![DATA, 0, DATA + 8, DATA + 16],
        ),
        ("InitOnceComplete", vec![DATA, 0, 0]),
    ] {
        v.mem.write_bytes(DATA, &[0xaa; 24]).unwrap();
        v.k("SetLastError", &[123]);
        assert_eq!(v.k(name, &args), 0, "{name}");
        assert_eq!(
            v.cpu.esp(),
            STACK_BASE + STACK_SIZE - 256 + 4 + args.len() as u32 * 4
        );
        assert_eq!(v.state.last_error, 120, "{name}");
        assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), 120);
        for offset in (0..24).step_by(4) {
            assert_eq!(v.mem.read_u32(DATA + offset).unwrap(), 0xaaaaaaaa, "{name}");
        }
    }
}

#[test]
fn timers_keep_pe32_abi_only_errors_and_independent_stack_sizes() {
    for (name, args, bytes) in [
        ("CreateWaitableTimerA", vec![0, 1, 0], 12),
        ("CreateWaitableTimerW", vec![0, 1, 0], 12),
        ("OpenWaitableTimerA", vec![2, 0, 0], 12),
        ("OpenWaitableTimerW", vec![2, 0, 0], 12),
        ("CreateWaitableTimerExA", vec![0, 0, 1, 2], 16),
        ("CreateWaitableTimerExW", vec![0, 0, 1, 2], 16),
        ("SetWaitableTimer", vec![0, DATA, 0, 0, 0, 0], 24),
        ("SetWaitableTimerEx", vec![0, DATA, 0, 0, 0, 0, 0], 28),
        ("CancelWaitableTimer", vec![0], 4),
    ] {
        let mut v = Vm::new();
        v.mem.write_u32(DATA, 0x12345678).unwrap();
        assert_eq!(v.k(name, &args), 0, "{name}");
        assert_eq!(v.state.last_error, 120, "{name}");
        assert_eq!(
            v.cpu.esp(),
            STACK_BASE + STACK_SIZE - 256 + 4 + bytes,
            "{name}"
        );
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0x12345678);
    }
}

#[test]
fn delays_keep_pe32_fault_before_yield_and_ignored_alertable() {
    for (name, args, result) in [
        ("Sleep", vec![0], 0),
        ("SleepEx", vec![0, u32::MAX], 0),
        ("SwitchToThread", vec![], 1),
    ] {
        let mut v = Vm::new();
        v.k("CreateThread", &[0, 0, CODE + 16, 0x98765432, 0, 0]);
        v.k("SetLastError", &[123]);
        let address = v.api.register("kernel32.dll", Some(name), None);
        let index = v.api.stub_index(address).unwrap();
        let sp = DATA + 4092;
        for (i, value) in args.into_iter().enumerate() {
            v.mem.write_u32(sp + 4 + i as u32 * 4, value).unwrap();
        }
        v.mem
            .virtual_protect(DATA, 4096, wire::mem::PAGE_NOACCESS)
            .unwrap();
        v.cpu.set_esp(sp);
        v.cpu.eip = address;
        v.cpu.regs[0] = 0x12345678;
        assert!(api::dispatch(
            index,
            &mut v.cpu,
            &mut v.mem,
            &mut v.api,
            &mut v.state,
            &mut v.log,
            &mut v.sched,
            1_000_000,
        )
        .is_err());
        assert_eq!(v.cpu.regs[0], result, "{name}");
        assert_eq!(v.cpu.esp(), sp);
        assert_eq!(v.cpu.eip, address);
        assert_eq!(v.sched.current, 0, "{name} must not switch after fault");
        assert_eq!(v.state.last_error, 123);
        assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), 123);
    }
}

#[test]
fn waits_keep_pe32_empty_arrays_count_clamp_and_extended_abi_only_policy() {
    let mut v = Vm::new();
    assert_eq!(v.k("WaitForMultipleObjects", &[0, 0, 1, 0]), 0);
    assert_eq!(v.k("WaitForMultipleObjects", &[0, 0, 0, 0]), 258);
    let event = v.k("CreateEventW", &[0, 1, 1, 0]);
    let array = DATA + 4096 - 64 * 4;
    for i in 0..64 {
        v.mem.write_u32(array + i * 4, event).unwrap();
    }
    v.mem
        .virtual_protect(DATA + 4096, 4096, wire::mem::PAGE_NOACCESS)
        .unwrap();
    assert_eq!(v.k("WaitForMultipleObjects", &[65, array, 1, 0]), 0);
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 0);
    assert_eq!(v.k("WaitForMultipleObjectsEx", &[0, 0, 0, 0, 0]), 0);
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 24);
}

#[test]
fn waits_keep_pe32_ignored_alertable_and_acquire_before_return_fault() {
    let mut v = Vm::new();
    let event = v.k("CreateEventW", &[0, 0, 1, 0]);
    assert_eq!(v.k("WaitForSingleObjectEx", &[event, 0, u32::MAX]), 0);
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 258);
    v.k("SetEvent", &[event]);
    let address = v
        .api
        .register("kernel32.dll", Some("WaitForSingleObjectEx"), None);
    let index = v.api.stub_index(address).unwrap();
    let sp = DATA + 4092;
    for (i, value) in [event, 0, u32::MAX].into_iter().enumerate() {
        v.mem.write_u32(sp + 4 + i as u32 * 4, value).unwrap();
    }
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_NOACCESS)
        .unwrap();
    v.cpu.set_esp(sp);
    v.cpu.eip = address;
    v.cpu.regs[0] = 0x12345678;
    assert!(api::dispatch(
        index,
        &mut v.cpu,
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        1_000_000,
    )
    .is_err());
    assert_eq!(v.cpu.regs[0], 0);
    assert_eq!(v.cpu.esp(), sp);
    assert_eq!(v.cpu.eip, address);
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 258);
}

#[test]
fn mutexes_keep_recursive_ownership_across_actual_guest_threads() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, b"wire-mutex\0").unwrap();
    v.wide(DATA + 128, "wire-mutex");
    let mutex = v.k("CreateMutexA", &[0, 1, DATA]);
    assert_eq!(v.k("WaitForSingleObject", &[mutex, 0]), 0);
    v.k("CreateThread", &[0, 0, CODE, 0, 0, 0]);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("ReleaseMutex", &[mutex]), 0);
    assert_eq!(v.state.last_error, 288);
    assert_eq!(v.k("WaitForSingleObject", &[mutex, 0]), 258);
    assert_eq!(v.k("CreateMutexW", &[0, 1, DATA + 128]), mutex);
    assert_eq!(v.state.last_error, 183);
    assert_eq!(v.k("WaitForSingleObject", &[mutex, 0]), 258);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("ReleaseMutex", &[mutex]), 1);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("WaitForSingleObject", &[mutex, 0]), 258);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("ReleaseMutex", &[mutex]), 1);
    assert_eq!(v.k("ReleaseMutex", &[mutex]), 0);
    assert_eq!(v.state.last_error, 288);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("WaitForSingleObject", &[mutex, 0]), 0);
    assert_eq!(v.k("ReleaseMutex", &[mutex]), 1);
}

#[test]
fn mutexes_keep_release_before_return_fault_and_abi_only_exports() {
    let mut v = Vm::new();
    let mutex = v.k("CreateMutexW", &[0, 1, 0]);
    v.k("CreateThread", &[0, 0, CODE, 0, 0, 0]);
    let address = v.api.register("kernel32.dll", Some("ReleaseMutex"), None);
    let index = v.api.stub_index(address).unwrap();
    let sp = DATA + 4092;
    v.mem.write_u32(sp + 4, mutex).unwrap();
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_NOACCESS)
        .unwrap();
    v.cpu.set_esp(sp);
    v.cpu.eip = address;
    assert!(api::dispatch(
        index,
        &mut v.cpu,
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        1_000_000,
    )
    .is_err());
    assert_eq!(v.cpu.regs[0], 1);
    assert_eq!(v.cpu.esp(), sp);
    assert_eq!(v.cpu.eip, address);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("WaitForSingleObject", &[mutex, 0]), 0);
    for (name, bytes) in [
        ("CreateMutexExA", 16),
        ("CreateMutexExW", 16),
        ("OpenMutexA", 12),
        ("OpenMutexW", 12),
    ] {
        assert!(api::native_export("kernel32.dll", name));
        assert_eq!(v.k(name, &[0, 0, 0, 0]), 0, "{name}");
        assert_eq!(v.state.last_error, 120, "{name}");
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4 + bytes);
    }
    for name in [
        "CreateMutexA",
        "CreateMutexW",
        "CreateMutexExA",
        "CreateMutexExW",
        "OpenMutexA",
        "OpenMutexW",
        "ReleaseMutex",
    ] {
        for dll in ["kernel32.dll", "kernelbase.dll"] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
        }
    }
}

#[test]
fn sync_objects_keep_pe32_semaphore_clamping_and_commit_before_output_fault() {
    let mut v = Vm::new();
    v.k("SetLastError", &[0x55]);
    let sem = v.k("CreateSemaphoreA", &[0, 1, 3, 0]);
    assert_ne!(sem, 0);
    assert_eq!(v.state.last_error, 0x55);
    assert_eq!(v.k("ReleaseSemaphore", &[sem, 5, DATA]), 1);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    assert_eq!(v.k("ReleaseSemaphore", &[sem, u32::MAX, DATA]), 1);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 3);
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_READONLY)
        .unwrap();
    v.cpu.regs[0] = 0x12345678;
    assert!(v
        .try_call("kernel32.dll", "ReleaseSemaphore", &[sem, 1, DATA])
        .is_err());
    assert_eq!(v.cpu.regs[0], 0x12345678);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 3);
    assert_eq!(v.state.last_error, 0x55);
    // The failing write follows the increment in the historical PE32 adapter.
    for _ in 0..3 {
        assert_eq!(v.k("WaitForSingleObject", &[sem, 0]), 0);
    }
    assert_eq!(v.k("WaitForSingleObject", &[sem, 0]), 258);
}

#[test]
fn sync_objects_keep_pe32_pulse_and_abi_only_stack_cleanup() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, b"wire-event\0").unwrap();
    let event = v.k("CreateEventA", &[0, 1, 1, DATA]);
    assert_eq!(v.k("OpenEventA", &[0, 0, DATA]), event);
    assert_eq!(v.k("PulseEvent", &[event]), 1);
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 258);
    for (name, bytes) in [
        ("CreateEventExA", 16),
        ("CreateEventExW", 16),
        ("CreateSemaphoreExA", 24),
        ("CreateSemaphoreExW", 24),
        ("OpenSemaphoreA", 12),
        ("OpenSemaphoreW", 12),
    ] {
        assert!(api::native_export("kernel32.dll", name), "{name}");
        assert_eq!(v.k(name, &[0, 0, 0, 0, 0, 0]), 0, "{name}");
        assert_eq!(v.state.last_error, 120, "{name}");
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4 + bytes);
    }
}

#[test]
fn sync_objects_keep_pe32_signal_before_bad_return_slot() {
    let mut v = Vm::new();
    let event = v.k("CreateEventW", &[0, 0, 0, 0]);
    let address = v.api.register("kernel32.dll", Some("SetEvent"), None);
    let index = v.api.stub_index(address).unwrap();
    let sp = DATA + 4092;
    v.mem.write_u32(sp + 4, event).unwrap();
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_NOACCESS)
        .unwrap();
    v.cpu.set_esp(sp);
    v.cpu.eip = address;
    assert!(api::dispatch(
        index,
        &mut v.cpu,
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        1_000_000,
    )
    .is_err());
    assert_eq!(v.cpu.regs[0], 1);
    assert_eq!(v.cpu.esp(), sp);
    assert_eq!(v.cpu.eip, address);
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 0);
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 258);
}

#[test]
fn shared_kernel_handle_namespace_keeps_named_identity_and_close_lifetimes() {
    let mut v = Vm::new();
    v.wide(DATA, "shared-object");
    let event = v.k("CreateEventW", &[0, 1, 0, DATA]);
    assert_eq!(event, 0x104); // Main thread owns the first legacy handle, 0x100.
    v.k("SetLastError", &[123]);
    // Preserve the existing cross-kind name lookup policy during refactoring.
    assert_eq!(v.k("CreateMutexW", &[0, 1, DATA]), event);
    assert_eq!(v.k("GetLastError", &[]), 183);
    assert_eq!(v.k("OpenEventW", &[0, 0, DATA]), event);
    assert_eq!(v.k("ReleaseMutex", &[event]), 0);
    assert_eq!(v.k("GetLastError", &[]), 6);
    assert_eq!(v.k("SetEvent", &[event]), 1);
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 0);
    assert_eq!(v.k("CloseHandle", &[event]), 1);
    assert_eq!(v.k("OpenEventW", &[0, 0, DATA]), 0);
    assert_eq!(v.k("GetLastError", &[]), 2);

    v.wide(DATA + 128, "C:\\handle.txt");
    let file = v.k("CreateFileW", &[DATA + 128, 0xc0000000, 0, 0, 2, 0x80, 0]);
    assert_eq!(file, 0x108);
    assert_eq!(v.k("FindClose", &[file]), 0);
    assert_eq!(v.k("GetLastError", &[]), 6);
    assert_eq!(v.k("GetFileType", &[file]), 1);
    assert_eq!(v.k("CloseHandle", &[file]), 1);
    assert_eq!(v.k("GetFileType", &[file]), 0);
    assert_eq!(v.k("CreateEventW", &[0, 0, 1, DATA]), 0x10c);
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), u32::MAX);
}

#[test]
fn thread_resource_limit_returns_win32_failure_and_does_not_create_partial_handles() {
    let mut v = Vm::new();
    let mut last_handle = 0x100;
    while v.sched.has_thread_capacity() {
        last_handle = v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
        assert_ne!(last_handle, 0);
    }
    let count = v.sched.threads.len();
    let next_tid_output = DATA;
    v.mem.write_u32(next_tid_output, 0xdeadbeef).unwrap();
    assert_eq!(v.k("CreateThread", &[0, 0, CODE, 0, 4, next_tid_output]), 0);
    assert_eq!(v.state.last_error, 8);
    assert_eq!(v.sched.threads.len(), count);
    assert_eq!(v.k("CreateEventW", &[0, 0, 0, 0]), last_handle + 4);
    assert_eq!(v.mem.read_u32(next_tid_output).unwrap(), 0xdeadbeef);
    for t in v.sched.threads.iter().skip(1) {
        assert!(t.stack_base + wire::thread::THREAD_STACK_SIZE < 0x10000000);
        assert!(t.teb < wire::loader::PEB_ADDR);
    }
}

#[test]
fn process_and_thread_affinity_match_single_cpu_profile_and_preserve_failed_outputs() {
    let mut v = Vm::new();
    v.mem.write_u32(DATA, 0xdeadbeef).unwrap();
    assert_eq!(
        v.k("GetProcessAffinityMask", &[u32::MAX, DATA, DATA + 4]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 1);
    v.mem.write_u32(DATA, 0xdeadbeef).unwrap();
    assert_eq!(v.k("GetProcessAffinityMask", &[u32::MAX, DATA, 0]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xdeadbeef);
    let h = v.k("OpenProcess", &[0x1600, 0, 4242]);
    assert_ne!(h, 0);
    assert_eq!(v.k("GetProcessId", &[h]), 4242);
    assert_eq!(v.k("GetProcessAffinityMask", &[h, DATA, DATA + 4]), 1);
    assert_eq!(v.k("SetProcessAffinityMask", &[h, 2]), 0);
    assert_eq!(v.k("SetProcessAffinityMask", &[h, 1]), 1);
    assert_eq!(v.k("GetExitCodeProcess", &[h, DATA]), 1);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 259);
    assert_eq!(v.k("CloseHandle", &[h]), 1);
    assert_eq!(v.k("GetProcessId", &[h]), 0);
    assert_eq!(v.state.last_error, 6);
    assert_eq!(v.k("SetThreadAffinityMask", &[0xfffffffe, 1]), 1);
    assert_eq!(v.k("SetThreadAffinityMask", &[0xfffffffe, 2]), 0);
    assert_eq!(v.k("SetThreadIdealProcessor", &[0xfffffffe, 0]), 0);
    assert_eq!(v.k("SetThreadIdealProcessor", &[0xfffffffe, 1]), u32::MAX);
    assert_eq!(v.k("GetActiveProcessorCount", &[0xffff]), 1);
}

#[test]
fn zero_delay_yields_finish_the_callers_frame_before_switching_threads() {
    for (name, args, cleanup, result) in [
        ("Sleep", vec![0], 8, 0),
        ("SleepEx", vec![0, 0], 12, 0),
        ("SwitchToThread", vec![], 4, 1),
    ] {
        let mut v = Vm::new();
        v.k("CreateThread", &[0, 0, CODE + 16, 0x98765432, 0, 0]);
        v.k("SetLastError", &[123]);
        let worker = v.sched.threads[1].ctx;
        let worker_teb = v.sched.threads[1].teb;
        v.mem.write_u32(worker_teb + 0x34, 456).unwrap();
        v.k(name, &args);
        assert_eq!(v.sched.current, 1, "{name}");
        assert_eq!(v.cpu.eip, worker.eip);
        assert_eq!(v.cpu.regs, worker.regs);
        assert_eq!(v.cpu.fs_base, worker_teb);
        assert_eq!(v.mem.read_u32(worker_teb + 0x34).unwrap(), 456);
        assert!(v.sched.round_robin(&mut v.cpu));
        assert_eq!(v.cpu.eip, RET, "{name} return PC");
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + cleanup);
        assert_eq!(v.cpu.regs[0], result);
        assert_eq!(v.cpu.fs_base, TEB_ADDR);
        assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), 123);
        // A single runnable thread still returns normally without switching.
        v.sched.threads[1].state = wire::thread::ThreadState::Suspended;
        assert_eq!(v.k(name, &args), 0);
        assert_eq!(v.sched.current, 0);
        assert_eq!(v.cpu.eip, RET);
    }
}

#[test]
fn thread_local_values_and_last_error_are_independent() {
    let mut v = Vm::new();
    let slot = v.k("TlsAlloc", &[]);
    let fls = v.k("FlsAlloc", &[0]);
    v.k("TlsSetValue", &[slot, 123]);
    v.k("FlsSetValue", &[fls, 456]);
    v.k("SetLastError", &[99]);
    let h = v.k("CreateThread", &[0, 0, CODE, 0, 0, 0]);
    assert_ne!(h, 0);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("GetLastError", &[]), 0);
    assert_eq!(v.k("TlsGetValue", &[slot]), 0);
    assert_eq!(v.k("FlsGetValue", &[fls]), 0);
    v.k("TlsSetValue", &[slot, 789]);
    v.k("SetLastError", &[88]);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("GetLastError", &[]), 99);
    assert_eq!(v.k("TlsGetValue", &[slot]), 123);
    assert_eq!(v.k("FlsGetValue", &[fls]), 456);
}

#[test]
fn wait_all_acquires_objects_atomically() {
    let mut v = Vm::new();
    let event = v.k("CreateEventW", &[0, 0, 1, 0]);
    let sem = v.k("CreateSemaphoreW", &[0, 0, 1, 0]);
    v.mem.write_u32(DATA, event).unwrap();
    v.mem.write_u32(DATA + 4, sem).unwrap();
    assert_eq!(v.k("WaitForMultipleObjects", &[2, DATA, 1, 0]), 0x102);
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 0);
    assert_eq!(v.k("SetEvent", &[event]), 1);
    assert_eq!(v.k("ReleaseSemaphore", &[sem, 1, 0]), 1);
    assert_eq!(v.k("WaitForMultipleObjects", &[2, DATA, 1, 0]), 0);
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 0x102);
    assert_eq!(v.k("WaitForSingleObject", &[sem, 0]), 0x102);
    assert_eq!(v.k("SetEvent", &[event]), 1);
    assert_eq!(v.k("WaitForSingleObjectEx", &[event, 0, 1]), 0);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 240); // stdcall: return + 3 arguments
    assert_eq!(v.k("WaitForSingleObjectEx", &[event, 0, 0]), 0x102);
    assert_eq!(v.k("WaitForSingleObjectEx", &[0xdead, 0, 0]), u32::MAX);
    assert_eq!(v.k("GetLastError", &[]), 6);
}

#[test]
fn blocked_thread_resumes_after_worker_signals_and_exits() {
    fn call(code: &mut Vec<u8>, stub: u32, args: &[u32]) {
        for &a in args.iter().rev() {
            code.push(0x68);
            code.extend(a.to_le_bytes());
        }
        code.push(0xb8);
        code.extend(stub.to_le_bytes());
        code.extend([0xff, 0xd0]);
    }
    let mut v = Vm::new();
    let event = v.k("CreateEventW", &[0, 0, 0, 0]);
    let set = v.api.register("kernel32.dll", Some("SetEvent"), None);
    let mut worker = Vec::new();
    call(&mut worker, set, &[event]);
    worker.extend([0xb8, 42, 0, 0, 0, 0xc2, 4, 0]);
    v.mem.write_bytes(CODE + 0x400, &worker).unwrap();
    let h = v.k("CreateThread", &[0, 0, CODE + 0x400, 0, 0, 0]);
    let wait = v
        .api
        .register("kernel32.dll", Some("WaitForSingleObject"), None);
    let get_exit = v
        .api
        .register("kernel32.dll", Some("GetExitCodeThread"), None);
    let exit = v.api.register("kernel32.dll", Some("ExitProcess"), None);
    let mut main = Vec::new();
    call(&mut main, wait, &[event, u32::MAX]);
    main.push(0xa3);
    main.extend((DATA + 16).to_le_bytes());
    call(&mut main, get_exit, &[h, DATA + 20]);
    call(&mut main, exit, &[0]);
    v.mem.write_bytes(CODE, &main).unwrap();
    v.cpu.eip = CODE;
    let outcome = v.cpu.run(
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        10_000,
    );
    assert!(matches!(outcome, wire::cpu::RunOutcome::Exited(0)));
    assert_eq!(v.mem.read_u32(DATA + 16).unwrap(), 0);
    assert_eq!(v.mem.read_u32(DATA + 20).unwrap(), 42);
}

#[test]
fn timeslice_wakes_expired_waiter_even_while_other_thread_spins() {
    use wire::thread::{ThreadState, WaitTarget, Wake};
    let mut v = Vm::new();
    v.k("CreateThread", &[0, 0, CODE, 0, 0, 0]);
    v.sched.threads[1].state = ThreadState::Waiting {
        target: WaitTarget::Sleep,
        deadline: Some(std::time::Instant::now()),
    };
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.sched.current, 1);
    assert_eq!(v.sched.take_wake(), Some(Wake::Completed(0x102)));
    assert_eq!(v.cpu.mxcsr, 0x1f80);
    assert_eq!(v.cpu.fpu.cw, 0x037f);
}

#[test]
fn worker_teb_slots_do_not_alias_main_teb_or_peb() {
    let mut v = Vm::new();
    v.mem.commit(wire::loader::PEB_ADDR, 0x1000).unwrap();
    v.mem.write_u32(wire::loader::PEB_ADDR, 0xcafe).unwrap();
    v.mem.write_u32(TEB_ADDR + 0x34, 0xbeef).unwrap();
    for _ in 0..40 {
        v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]);
    }
    assert_eq!(v.mem.read_u32(wire::loader::PEB_ADDR).unwrap(), 0xcafe);
    assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), 0xbeef);
    for t in v.sched.threads.iter().skip(1) {
        assert_ne!(t.teb, TEB_ADDR);
        assert_ne!(t.teb, wire::loader::PEB_ADDR);
        assert_eq!(v.mem.read_u32(t.teb + 0x24).unwrap(), t.tid);
    }
}

#[test]
fn x87_stack_and_rounding_survive_thread_switches() {
    let mut v = Vm::new();
    v.k("CreateThread", &[0, 0, CODE + 0x400, 0, 0, 0]);
    v.mem.write_bytes(CODE, &[0xd9, 0xe8]).unwrap();
    v.cpu.eip = CODE;
    v.cpu.step(&mut v.mem).unwrap();
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.cpu.fpu.tags, 0xffff);
    v.cpu.fpu.cw = 0xb7f;
    v.mem.write_bytes(CODE + 0x400, &[0xd9, 0xeb]).unwrap();
    v.cpu.step(&mut v.mem).unwrap();
    let worker = v.cpu.fpu;
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.cpu.fpu.st_bits(0), 0x3fff_8000_0000_0000_0000);
    assert_eq!(v.cpu.fpu.cw, 0x37f);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.cpu.fpu.regs, worker.regs);
    assert_eq!(v.cpu.fpu.tags, worker.tags);
    assert_eq!(v.cpu.fpu.sw, worker.sw);
    assert_eq!(v.cpu.fpu.cw, worker.cw);
}
