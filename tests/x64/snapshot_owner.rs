//! Independent Toolhelp lifetime, snapshot isolation and copyout baselines.
use super::*;
use wire::x64::runtime::Runtime64;

fn error(mem: &mut Memory, cpu: &mut Cpu64, rt: &mut Runtime64) -> u64 {
    startup_call(mem, cpu, rt, "GetLastError", &[])
}

fn fault(mem: &mut Memory, cpu: &mut Cpu64, rt: &mut Runtime64, name: &str, args: &[u64]) {
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    for (index, value) in args.iter().enumerate() {
        cpu.regs[[1, 2, 8, 9][index]] = *value;
    }
    let before = (cpu.rip, cpu.regs, rt.api_calls);
    assert!(rt.dispatch("kernel32.dll", name, cpu, mem).is_err());
    assert_eq!((cpu.rip, cpu.regs, rt.api_calls), before);
}

#[test]
fn snapshots_preserve_handle_flags_validation_order_and_monotonic_limits() {
    let (mut mem, mut cpu, mut rt) = startup();
    let out = STACK + 0x100;
    let mut live = Vec::new();
    for index in 0..128 {
        let handle = startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "CreateToolhelp32Snapshot",
            &[
                0x100000002 | if index == 0 { 0x80000000 } else { 0 },
                u64::MAX,
            ],
        );
        assert_eq!(handle, 0x7fff1000 + index);
        live.push(handle);
    }
    for (flags, expected) in [(8, 50), (2, 8)] {
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "CreateToolhelp32Snapshot",
                &[flags, 0]
            ),
            u64::MAX
        );
        assert_eq!(error(&mut mem, &mut cpu, &mut rt), expected);
    }
    let first = live[0];
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetHandleInformation",
            &[first, out]
        ),
        1
    );
    assert_eq!(mem.read_int(out, 4).unwrap(), 1);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SetHandleInformation",
            &[first, 4, 0]
        ),
        0
    );
    assert_eq!(error(&mut mem, &mut cpu, &mut rt), 87);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SetHandleInformation",
            &[first, 0x100000002, 0x100000002]
        ),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "CloseHandle", &[first]),
        0
    );
    assert_eq!(error(&mut mem, &mut cpu, &mut rt), 5); // Token protection instead returns 6.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "Process32FirstW",
            &[first | 0x100000000, 1]
        ),
        0
    );
    assert_eq!(error(&mut mem, &mut cpu, &mut rt), 6); // Handle validation precedes output read.
    startup_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "SetHandleInformation",
        &[first, 2, 0],
    );
    for handle in live {
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "CloseHandle", &[handle]),
            1
        );
    }
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "Process32FirstW", &[first, 1]),
        0
    );
    assert_eq!(error(&mut mem, &mut cpu, &mut rt), 6);
    for index in 128..4096 {
        let handle = startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "CreateToolhelp32Snapshot",
            &[2, 0],
        );
        assert_eq!(handle, 0x7fff1000 + index);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "CloseHandle", &[handle]),
            1
        );
    }
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "CreateToolhelp32Snapshot",
            &[2, 0]
        ),
        u64::MAX
    );
    assert_eq!(error(&mut mem, &mut cpu, &mut rt), 8);
}

#[test]
fn snapshot_copyout_faults_keep_shared_cursors_and_saved_return_frame() {
    let (mut mem, mut cpu, mut rt) = startup();
    let out = STACK + 0x100;
    let snap = startup_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "CreateToolhelp32Snapshot",
        &[2, 0],
    );
    mem.write_int(out, 4, 304).unwrap();
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "Process32First", &[snap, out]),
        1
    );
    mem.write_int(out, 4, 568).unwrap();
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "Process32NextW", &[snap, out]),
        0
    );
    assert_eq!(error(&mut mem, &mut cpu, &mut rt), 18); // ANSI/wide share process_done.
    let partial = STACK + 8192 - 4;
    mem.write_int(partial, 4, 568).unwrap();
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "Process32FirstW",
        &[snap, partial],
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "Process32NextW", &[snap, out]),
        0
    );
    assert_eq!(error(&mut mem, &mut cpu, &mut rt), 18);
    let guarded = 0x570000000;
    mem.map(guarded, 4096, READ | WRITE).unwrap();
    mem.write_int(guarded, 4, 28).unwrap();
    mem.virtual_protect(guarded, 4096, 4 | wire::mem::PAGE_GUARD)
        .unwrap();
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "Thread32First",
        &[snap, guarded],
    );
    assert_eq!(mem.read_int(guarded, 4).unwrap(), 28);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "Thread32First",
            &[snap, guarded]
        ),
        0
    );
    assert_eq!(error(&mut mem, &mut cpu, &mut rt), 18); // Size read precedes unsupported list.
    mem.write_int(guarded, 4, 27).unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "Thread32First",
            &[snap, guarded]
        ),
        0
    );
    assert_eq!(error(&mut mem, &mut cpu, &mut rt), 24);
    mem.map(STACK + 8192, 4096, READ | WRITE).unwrap();
    let alias = STACK + 0x1f80;
    mem.write_int(alias, 4, 568).unwrap();
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "Process32FirstW",
            &[snap, alias]
        ),
        1
    );
    assert_eq!(mem.read_int(alias + 8, 8).unwrap(), 1); // PID overwrites the saved return slot.
    assert_eq!(error(&mut mem, &mut cpu, &mut rt), 123);
}

#[test]
fn snapshot_threads_and_priorities_are_captured_at_creation() {
    let (mut mem, mut cpu, mut rt) = startup();
    rt.enable_scheduler();
    let out = STACK + 0x100;
    let create = |mem: &mut Memory, cpu: &mut Cpu64, rt: &mut Runtime64| {
        startup_call(mem, cpu, rt, "CreateThread", &[0, 0, BASE, 0, 4, 0])
    };
    let thread = create(&mut mem, &mut cpu, &mut rt);
    assert_ne!(thread, 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SetThreadPriority",
            &[thread, (-15i64) as u64]
        ),
        1
    );
    let snap = startup_call(
        &mut mem,
        &mut cpu,
        &mut rt,
        "CreateToolhelp32Snapshot",
        &[4, 0],
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SetThreadPriority",
            &[thread, 15]
        ),
        1
    );
    assert_ne!(create(&mut mem, &mut cpu, &mut rt), 0);
    mem.write_int(out, 4, 28).unwrap();
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "Thread32Next", &[snap, out]),
        1
    ); // Next also starts at zero.
    assert_eq!(mem.read_int(out + 8, 4).unwrap(), 1);
    let partial = STACK + 8192 - 4;
    mem.write_int(partial, 4, 28).unwrap();
    fault(
        &mut mem,
        &mut cpu,
        &mut rt,
        "Thread32First",
        &[snap, partial],
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "Thread32Next", &[snap, out]),
        1
    );
    assert_eq!(mem.read_int(out + 8, 4).unwrap(), 2);
    assert_eq!(mem.read_int(out + 16, 4).unwrap(), 1); // Captured -15 priority, before it became +15.
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "Thread32Next", &[snap, out]),
        0
    );
    assert_eq!(error(&mut mem, &mut cpu, &mut rt), 18); // Later third thread is absent.
}
