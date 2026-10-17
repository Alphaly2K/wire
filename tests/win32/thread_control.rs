//! PE32 creation commits before ID copy-out; optional controls remain ABI-only.
use super::*;
use wire::thread::ThreadState;

#[test]
fn thread_control_keeps_pe32_creation_ignored_fields_and_late_output_fault() {
    let mut v = Vm::new();
    v.k("SetLastError", &[123]);
    let h = v.k(
        "CreateThread",
        &[0xdead0000, u32::MAX, CODE, 0x98765432, 0xfffffffb, DATA],
    );
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 2);
    assert_eq!(v.sched.threads[1].handle, h);
    assert_eq!(v.sched.threads[1].state, ThreadState::Runnable);
    assert_eq!(v.state.last_error, 123);
    let count = v.sched.threads.len();
    assert!(v
        .try_call(
            "kernel32.dll",
            "CreateThread",
            &[0, 0, CODE, 7, u32::MAX, 0xdead0000]
        )
        .is_err());
    assert_eq!(v.sched.threads.len(), count + 1);
    assert_eq!(
        v.sched.threads.last().unwrap().state,
        ThreadState::Suspended
    );
    assert_eq!(v.sched.threads.last().unwrap().tid, 3);
    let next = v.k("CreateThread", &[0, 0, CODE, 0, 4, DATA]);
    assert_eq!(next, h + 8);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 4);
    // PE32 re-reads the return address after writing the ID.
    v.k(
        "CreateThread",
        &[0, 0, CODE, 0, 4, STACK_BASE + STACK_SIZE - 256],
    );
    assert_eq!(v.cpu.eip, 5);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 228);
}

#[test]
fn thread_control_keeps_pe32_exit_argument_and_worker_eax_paths() {
    for (dll, eax, arg, expected) in [("kernel32.dll", 42, 7, 7), ("wire", 42, 7, 42)] {
        let mut v = Vm::new();
        v.cpu.regs[0] = eax;
        v.call(dll, "ExitThread", &[arg]);
        assert_eq!(v.sched.threads[0].state, ThreadState::Exited(expected));
        assert!(v.cpu.halted);
        assert_eq!(v.cpu.exit_code, Some(expected));
    }
}

#[test]
fn thread_control_keeps_pe32_optional_abi_and_unknown_provider_cleanup() {
    let mut v = Vm::new();
    for (name, args, bytes) in [
        ("QueueUserAPC", vec![0xdead0000, 0, DATA], 12),
        ("GetThreadContext", vec![0, DATA], 8),
        ("SetThreadDescription", vec![0, DATA], 8),
        ("GetThreadDescription", vec![0, DATA], 8),
    ] {
        v.mem.write_u32(DATA, 0xabcdef12).unwrap();
        assert_eq!(v.k(name, &args), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252 + bytes);
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xabcdef12);
    }
    for dll in ["kernelbase.dll", "user32.dll"] {
        for name in [
            "CreateThread",
            "ExitThread",
            "QueueUserAPC",
            "GetThreadContext",
            "SetThreadDescription",
            "GetThreadDescription",
        ] {
            assert_eq!(v.call(dll, name, &[0, 0, CODE, 0, 0, DATA]), 0);
            assert_eq!(v.state.last_error, 120);
            assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252);
            assert_eq!(v.sched.threads.len(), 1);
        }
    }
}
