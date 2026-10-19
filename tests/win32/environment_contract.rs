//! Preserve the intentionally empty PE32 environment and scratch lifetime.
use super::*;

#[test]
fn environment_pe32_keeps_ignored_pointers_empty_blocks_and_abi_only_exports() {
    let mut v = Vm::new();
    v.k("SetLastError", &[77]);
    for name in ["GetEnvironmentVariableA", "GetEnvironmentVariableW"] {
        assert_eq!(v.k(name, &[0x30000, 0x30000, 100]), 0);
        assert_eq!(v.state.last_error, 77);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 16);
        assert!(v
            .log
            .recent
            .back()
            .unwrap()
            .ends_with(" ; empty environment"));
    }
    let top = v.state.scratch_position();
    for (i, name) in ["GetEnvironmentStrings", "GetEnvironmentStringsW"]
        .into_iter()
        .enumerate()
    {
        let p = v.k(name, &[]);
        assert_eq!(p, top + 16 * i as u32);
        assert_eq!(v.mem.read_u32(p).unwrap(), 0);
        assert_eq!(v.state.last_error, 77);
        assert!(v
            .log
            .recent
            .back()
            .unwrap()
            .ends_with(" ; empty environment block"));
    }
    for name in ["FreeEnvironmentStringsA", "FreeEnvironmentStringsW"] {
        assert_eq!(v.k(name, &[0xdeadbeef]), 1);
        assert_eq!(v.state.scratch_position(), top + 32);
        assert_eq!(v.state.last_error, 77);
    }
    for name in [
        "SetEnvironmentVariableA",
        "SetEnvironmentVariableW",
        "GetEnvironmentStringsA",
    ] {
        assert_eq!(v.k(name, &[0x30000, 0x30000]), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(
            v.cpu.esp(),
            STACK_BASE + STACK_SIZE - 256 + if name.starts_with("Set") { 12 } else { 4 }
        );
        let line = v.log.recent.back().unwrap();
        assert!(line.contains("[UNIMPLEMENTED]"));
        assert!(!line.contains("unknown stdcall arity"));
    }
}

#[test]
fn environment_pe32_keeps_scratch_exhaustion_fault_before_error_export() {
    let mut v = Vm::new();
    v.k("SetLastError", &[77]);
    let remaining = 0x01000000 - v.state.scratch_position();
    assert_ne!(v.state.scratch_alloc(&mut v.mem, remaining, 1), 0);
    let calls = v.log.call_count;
    assert!(v
        .try_call("kernel32.dll", "GetEnvironmentStringsW", &[])
        .is_err());
    assert_eq!(v.state.last_error, 8);
    assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), 77);
    assert_eq!(v.log.call_count, calls);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256);
}
