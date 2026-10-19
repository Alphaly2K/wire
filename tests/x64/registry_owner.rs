//! Registry lifetime and copyout baselines, independent of its implementation.
use super::*;
use wire::x64::runtime::{Runtime64, TEB};

const HKCU: u64 = 0xffffffff80000001;

fn call(mem: &mut Memory, cpu: &mut Cpu64, rt: &mut Runtime64, name: &str, args: &[u64]) -> u64 {
    startup_dll_call(mem, cpu, rt, "advapi32.dll", name, args)
}

fn fault(mem: &mut Memory, cpu: &mut Cpu64, rt: &mut Runtime64, name: &str, args: &[u64]) {
    cpu.regs[4] = STACK + 0x1f88;
    mem.write_int(cpu.regs[4], 8, BASE).unwrap();
    for (i, value) in args.iter().enumerate() {
        if i < 4 {
            cpu.regs[[1, 2, 8, 9][i]] = *value;
        } else {
            mem.write_int(cpu.regs[4] + 40 + (i as u64 - 4) * 8, 8, *value)
                .unwrap();
        }
    }
    let before = (cpu.rip, cpu.regs, rt.api_calls);
    assert!(rt.dispatch("advapi32.dll", name, cpu, mem).is_err());
    assert_eq!((cpu.rip, cpu.regs, rt.api_calls), before);
}

#[test]
fn registry_closed_handles_are_not_reused_and_lifetime_exhaustion_precedes_copyout() {
    let (mut mem, mut cpu, mut rt) = startup();
    let out = STACK + 0x200;
    // A failed output must not consume the first handle.
    fault(&mut mem, &mut cpu, &mut rt, "RegOpenKeyA", &[HKCU, 0, 1]);
    for index in 0..4096 {
        assert_eq!(
            call(&mut mem, &mut cpu, &mut rt, "RegOpenKeyA", &[HKCU, 0, out]),
            0
        );
        let handle = mem.read_int(out, 8).unwrap();
        assert_eq!(handle, TEB + 0x500000 + index * 16);
        assert_eq!(
            call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "RegCloseKey",
                &[handle as u32 as u64]
            ),
            6
        );
        assert_eq!(
            call(&mut mem, &mut cpu, &mut rt, "RegCloseKey", &[handle]),
            0
        );
        assert_eq!(
            call(&mut mem, &mut cpu, &mut rt, "RegCloseKey", &[handle]),
            6
        );
    }
    mem.write_int(out, 8, 0xfeed).unwrap();
    for output in [out, 1] {
        assert_eq!(
            call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "RegOpenKeyA",
                &[HKCU, 0, output]
            ),
            8
        );
    }
    assert_eq!(mem.read_int(out, 8).unwrap(), 0xfeed);
    assert_eq!(call(&mut mem, &mut cpu, &mut rt, "RegCloseKey", &[HKCU]), 0);
}

#[test]
fn registry_query_keeps_alias_write_order_and_guard_preflight_before_outputs() {
    let (mut mem, mut cpu, mut rt) = startup();
    let kind = 0x550000000;
    let size = kind + 4096;
    let data = size + 4096;
    mem.map(kind, 12288, READ | WRITE).unwrap();
    mem.write(data, &[0x12, 0x34, 0x56, 0x78]).unwrap();
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "RegSetValueExA",
            &[HKCU, 0, 0, 3, data, 4]
        ),
        0
    );
    mem.write_int(kind, 4, 0xa5a5a5a5).unwrap();
    mem.write_int(size, 4, 4).unwrap();
    mem.write_int(data, 4, 0xcccccccc).unwrap();
    for output in [kind, size, data] {
        mem.virtual_protect(output, 4096, 4 | wire::mem::PAGE_GUARD)
            .unwrap();
    }
    // Capacity is read first, followed by type and data preflight. No output
    // commits until all three have succeeded, even across one-shot guard faults.
    for _ in 0..3 {
        fault(
            &mut mem,
            &mut cpu,
            &mut rt,
            "RegQueryValueExA",
            &[HKCU, 0, 0, kind, data, size],
        );
    }
    assert_eq!(mem.read_int(kind, 4).unwrap(), 0xa5a5a5a5);
    assert_eq!(mem.read_int(size, 4).unwrap(), 4);
    assert_eq!(mem.read_int(data, 4).unwrap(), 0xcccccccc);
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "RegQueryValueExA",
            &[HKCU, 0, 0, kind, data, size]
        ),
        0
    );
    assert_eq!(mem.read_int(kind, 4).unwrap(), 3);
    assert_eq!(mem.read_int(size, 4).unwrap(), 4);
    assert_eq!(mem.read_int(data, 4).unwrap(), 0x78563412);
    // Type then required size then payload: all may alias the original capacity.
    mem.write_int(size, 4, 4).unwrap();
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "RegQueryValueExA",
            &[HKCU, 0, 0, size, size, size]
        ),
        0
    );
    assert_eq!(mem.read_int(size, 4).unwrap(), 0x78563412);
    mem.write_int(size, 4, 1).unwrap();
    assert_eq!(
        call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "RegQueryValueExA",
            &[HKCU, 0, 0, size, size, size]
        ),
        234
    );
    assert_eq!(mem.read_int(size, 4).unwrap(), 4);
}
