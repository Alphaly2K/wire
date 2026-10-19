//! Guest-owned Win64 variables, output faults and environment block leases.
use super::*;

#[test]
fn environment_win64_keeps_errors_dword_capacity_and_failed_set_value() {
    let (mut mem, mut cpu, mut rt) = startup();
    let name = STACK + 0x100;
    let value = STACK + 0x200;
    let out = STACK + 0x300;
    mem.write(name, b"name\0").unwrap();
    mem.write(value, b"old\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SetEnvironmentVariableA",
            &[name, value]
        ),
        1
    );
    cpu.regs[1] = name;
    cpu.regs[2] = 1;
    let frame = (cpu.rip, cpu.regs[4], rt.api_calls);
    assert!(rt
        .dispatch(
            "kernel32.dll",
            "SetEnvironmentVariableA",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!((cpu.rip, cpu.regs[4], rt.api_calls), frame);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetEnvironmentVariableA",
            &[name, 1, 0x100000002]
        ),
        4
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetEnvironmentVariableA",
            &[name, out, 4]
        ),
        3
    );
    assert_eq!(mem.read_int(out, 4).unwrap(), 0x00646c6f);
    cpu.regs[1] = name;
    cpu.regs[2] = 1;
    cpu.regs[8] = 4;
    let frame = (cpu.rip, cpu.regs[4], rt.api_calls);
    assert!(rt
        .dispatch(
            "kernel32.dll",
            "GetEnvironmentVariableA",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!((cpu.rip, cpu.regs[4], rt.api_calls), frame);
    mem.write(name, b"bad=name\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "GetEnvironmentVariableA",
            &[name, 1, 100]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    mem.write(name, b"missing\0").unwrap();
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "SetEnvironmentVariableA",
            &[name, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        203
    );
}

#[test]
fn environment_win64_keeps_block_lifetime_cross_free_and_absent_ansi_block() {
    let (mut mem, mut cpu, mut rt) = startup();
    let heap = startup_call(&mut mem, &mut cpu, &mut rt, "GetProcessHeap", &[]);
    let block = startup_call(&mut mem, &mut cpu, &mut rt, "GetEnvironmentStringsW", &[]);
    assert_eq!(mem.read_int(block, 4).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "FreeEnvironmentStringsW",
            &[block]
        ),
        1
    );
    assert!(mem.read_int(block, 4).is_err());
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "FreeEnvironmentStringsW",
            &[block]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        87
    );
    let block = startup_call(&mut mem, &mut cpu, &mut rt, "GetEnvironmentStringsW", &[]);
    cpu.regs[1] = heap;
    cpu.regs[2] = 0;
    cpu.regs[8] = block;
    let frame = (cpu.rip, cpu.regs[4], rt.api_calls);
    assert!(rt
        .dispatch("kernel32.dll", "HeapFree", &mut cpu, &mut mem)
        .unwrap_err()
        .contains("invalid Win64 heap allocation"));
    assert_eq!((cpu.rip, cpu.regs[4], rt.api_calls), frame);
    assert_eq!(mem.read_int(block, 4).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "FreeEnvironmentStringsW",
            &[block]
        ),
        1
    );
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        for name in [
            "GetEnvironmentStrings",
            "GetEnvironmentStringsA",
            "FreeEnvironmentStringsA",
        ] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
        }
        for name in [
            "GetEnvironmentStringsW",
            "FreeEnvironmentStringsW",
            "GetEnvironmentVariableA",
            "GetEnvironmentVariableW",
            "SetEnvironmentVariableA",
            "SetEnvironmentVariableW",
        ] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            assert!(!wire::x64::runtime::import_supported("user32.dll", name));
        }
    }
}
