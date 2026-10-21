//! Independent PE32 scheduler, ABI-only and unsaved-return behavior.
use super::*;
use wire::thread::ThreadState;

#[test]
fn thread_properties_keep_pe32_state_switches_priority_noop_and_affinity() {
    let mut v = Vm::new();
    let handle = v.sched.threads[0].handle;
    v.k("SetLastError", &[123]);
    for state in [
        ThreadState::Runnable,
        ThreadState::Suspended,
        ThreadState::Exited(42),
    ] {
        v.sched.threads[0].state = state.clone();
        assert_eq!(v.k("SuspendThread", &[handle]), 0);
        assert_eq!(
            v.sched.threads[0].state,
            if state == ThreadState::Runnable {
                ThreadState::Suspended
            } else {
                state.clone()
            }
        );
        assert_eq!(v.k("SuspendThread", &[handle]), 0);
        assert_eq!(v.k("ResumeThread", &[handle]), 0);
        assert_eq!(
            v.sched.threads[0].state,
            if matches!(state, ThreadState::Runnable | ThreadState::Suspended) {
                ThreadState::Runnable
            } else {
                state
            }
        );
        assert_eq!(v.k("ResumeThread", &[handle]), 0);
    }
    for h in [0, u32::MAX - 1, handle] {
        assert_eq!(v.k("SetThreadPriority", &[h, 0x80000000]), 1);
        assert_eq!(v.k("GetThreadPriority", &[h]), 0);
        assert_eq!(v.state.last_error, 123);
    }
    assert_eq!(v.k("SetThreadAffinityMask", &[u32::MAX - 1, 1]), 1);
    assert_eq!(v.k("SetThreadAffinityMask", &[handle, 1]), 1);
    assert_eq!(v.k("SetThreadAffinityMask", &[0, 0]), 0);
    assert_eq!(v.state.last_error, 6);
    assert_eq!(v.k("SetThreadAffinityMask", &[u32::MAX - 1, 2]), 0);
    assert_eq!(v.state.last_error, 87);
    for name in ["SuspendThread", "ResumeThread"] {
        assert_eq!(v.k(name, &[u32::MAX - 1]), u32::MAX);
        assert_eq!(v.state.last_error, 6);
    }
}

#[test]
fn thread_properties_keep_pe32_exit_output_and_unsaved_return_alias() {
    let mut v = Vm::new();
    let handle = v.sched.threads[0].handle;
    for (state, expected) in [
        (ThreadState::Runnable, 259),
        (ThreadState::Suspended, 259),
        (ThreadState::Exited(42), 42),
    ] {
        v.sched.threads[0].state = state;
        assert_eq!(v.k("GetExitCodeThread", &[handle, DATA]), 1);
        assert_eq!(v.mem.read_u32(DATA).unwrap(), expected);
    }
    for h in [0, u32::MAX - 1] {
        v.k("SetLastError", &[123]);
        assert_eq!(v.k("GetExitCodeThread", &[h, DATA]), 1);
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0);
        assert_eq!(v.state.last_error, 123);
    }
    let slot = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.k("GetExitCodeThread", &[handle, slot]), 1);
    assert_eq!(v.cpu.eip, 42);
    assert_eq!(v.cpu.esp(), slot + 12);
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_NOACCESS)
        .unwrap();
    v.cpu.regs[0] = 0x9876;
    assert!(v
        .try_call("kernel32.dll", "GetExitCodeThread", &[0, DATA])
        .is_err());
    assert_eq!(v.cpu.regs[0], 0x9876);
    assert_eq!(v.cpu.esp(), slot);
}

#[test]
fn thread_properties_keep_pe32_abi_only_and_wrong_provider_cleanup() {
    let mut v = Vm::new();
    for (name, args, bytes) in [
        ("GetThreadId", vec![u32::MAX - 1], 4),
        ("OpenThread", vec![0x1fffff, 1, 1], 12),
        ("GetThreadErrorMode", vec![], 0),
        ("SetThreadErrorMode", vec![0x8007, DATA], 8),
    ] {
        v.mem.write_u32(DATA, 0xabcdef12).unwrap();
        assert_eq!(v.k(name, &args), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252 + bytes);
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xabcdef12);
    }
    for dll in ["kernelbase.dll", "user32.dll"] {
        for name in [
            "GetThreadId",
            "SetThreadErrorMode",
            "GetThreadErrorMode",
            "SuspendThread",
            "ResumeThread",
            "SetThreadPriority",
            "GetThreadPriority",
            "GetExitCodeThread",
            "SetThreadAffinityMask",
        ] {
            assert_eq!(v.call(dll, name, &[0, DATA]), 0);
            assert_eq!(v.state.last_error, 120);
            assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252);
        }
    }
}
