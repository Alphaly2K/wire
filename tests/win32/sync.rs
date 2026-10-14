//! Existing behavioral tests grouped by sync; expectations remain independent of API catalogs.

use super::*;

#[test]
fn thread_resource_limit_returns_win32_failure_and_does_not_create_partial_handles() {
    let mut v = Vm::new();
    while v.sched.has_thread_capacity() {
        assert_ne!(v.k("CreateThread", &[0, 0, CODE, 0, 4, 0]), 0);
    }
    let count = v.sched.threads.len();
    let handles = v.state.handles.len();
    let next_tid_output = DATA;
    v.mem.write_u32(next_tid_output, 0xdeadbeef).unwrap();
    assert_eq!(v.k("CreateThread", &[0, 0, CODE, 0, 4, next_tid_output]), 0);
    assert_eq!(v.state.last_error, 8);
    assert_eq!(v.sched.threads.len(), count);
    assert_eq!(v.state.handles.len(), handles);
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
    assert!(matches!(
        v.state.handles.get(&event),
        Some(HandleObj::Event { signaled: true, .. })
    ));
    assert_eq!(v.k("ReleaseSemaphore", &[sem, 1, 0]), 1);
    assert_eq!(v.k("WaitForMultipleObjects", &[2, DATA, 1, 0]), 0);
    assert!(matches!(
        v.state.handles.get(&event),
        Some(HandleObj::Event {
            signaled: false,
            ..
        })
    ));
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
