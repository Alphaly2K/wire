//! Independent legacy layouts and fault ordering, fixed before adapter migration.
use super::*;

#[test]
fn critical_sections_keep_pe32_block_retest_and_wake_after_final_release() {
    use wire::thread::{ThreadState, WaitTarget, Wake};
    let mut v = Vm::new();
    v.k("InitializeCriticalSection", &[DATA]);
    v.k("EnterCriticalSection", &[DATA]);
    v.k("EnterCriticalSection", &[DATA]);
    v.k("CreateThread", &[0, 0, CODE, 0, 0, 0]);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.k("TryEnterCriticalSection", &[DATA]), 0);
    v.k("EnterCriticalSection", &[DATA]);
    assert_eq!(v.sched.current, 0);
    assert!(matches!(
        v.sched.threads[1].state,
        ThreadState::Waiting {
            target: WaitTarget::CriticalSection(DATA),
            ..
        }
    ));
    v.k("LeaveCriticalSection", &[DATA]);
    assert!(matches!(
        v.sched.threads[1].state,
        ThreadState::Waiting { .. }
    ));
    v.k("LeaveCriticalSection", &[DATA]);
    assert_eq!(v.sched.threads[1].state, ThreadState::Runnable);
    assert!(v.sched.round_robin(&mut v.cpu));
    assert_eq!(v.sched.take_wake(), Some(Wake::Retest));
    assert_eq!(v.k("TryEnterCriticalSection", &[DATA]), 1);
    assert_eq!(v.mem.read_u32(DATA + 12).unwrap(), v.sched.current_tid());
}

#[test]
fn critical_sections_keep_pe32_layout_permissive_release_and_spin_abi_only() {
    let mut v = Vm::new();
    for (name, args, result, spin) in [
        ("InitializeCriticalSection", vec![DATA], 0, 0),
        (
            "InitializeCriticalSectionAndSpinCount",
            vec![DATA, 500],
            1,
            500,
        ),
        (
            "InitializeCriticalSectionEx",
            vec![DATA, 900, u32::MAX],
            1,
            900,
        ),
    ] {
        v.k("SetLastError", &[123]);
        assert_eq!(v.k(name, &args), result);
        for (offset, expected) in [(0, 0), (4, u32::MAX), (8, 0), (12, 0), (16, 0), (20, spin)] {
            assert_eq!(
                v.mem.read_u32(DATA + offset).unwrap(),
                expected,
                "{name}+{offset}"
            );
        }
        assert_eq!(v.k("TryEnterCriticalSection", &[DATA]), 1);
        assert_eq!(v.k("EnterCriticalSection", &[DATA]), 0);
        assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 2);
        assert_eq!(v.mem.read_u32(DATA + 12).unwrap(), v.sched.current_tid());
        assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 0);
        v.k("LeaveCriticalSection", &[DATA]);
        v.k("DeleteCriticalSection", &[DATA]); // Historically allowed while owned.
        assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 1);
        assert_eq!(v.mem.read_u32(DATA + 12).unwrap(), 0);
        v.k("LeaveCriticalSection", &[DATA]);
        v.k("LeaveCriticalSection", &[DATA]); // Saturates at zero without owner check.
        assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 0);
        assert_eq!(v.state.last_error, 123);
        assert_eq!(v.k("SetCriticalSectionSpinCount", &[DATA, 44]), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.mem.read_u32(DATA + 20).unwrap(), spin);
    }
}

#[test]
fn critical_sections_keep_pe32_partial_field_writes_on_guest_fault() {
    let mut v = Vm::new();
    let section = DATA + 4096 - 12;
    v.mem.write_bytes(section, &[0xaa; 24]).unwrap();
    v.mem
        .virtual_protect(DATA + 4096, 4096, wire::mem::PAGE_NOACCESS)
        .unwrap();
    assert!(v
        .try_call("kernel32.dll", "InitializeCriticalSection", &[section])
        .is_err());
    assert_eq!(v.mem.read_u32(section).unwrap(), 0);
    assert_eq!(v.mem.read_u32(section + 4).unwrap(), u32::MAX);
    assert_eq!(v.mem.read_u32(section + 8).unwrap(), 0);
    v.mem
        .virtual_protect(DATA + 4096, 4096, wire::mem::PAGE_READWRITE)
        .unwrap();
    v.k("InitializeCriticalSection", &[section]);
    v.k("EnterCriticalSection", &[section]);
    let owner = v.mem.read_u32(section + 12).unwrap();
    v.mem
        .virtual_protect(DATA + 4096, 4096, wire::mem::PAGE_READONLY)
        .unwrap();
    assert!(v
        .try_call("kernel32.dll", "LeaveCriticalSection", &[section])
        .is_err());
    assert_eq!(v.mem.read_u32(section + 8).unwrap(), 0);
    assert_eq!(v.mem.read_u32(section + 12).unwrap(), owner);
    assert_eq!(v.mem.read_u32(section + 4).unwrap(), 0);
}

#[test]
fn critical_sections_keep_pe32_acquire_before_return_fault() {
    let mut v = Vm::new();
    let section = DATA + 8192;
    v.k("InitializeCriticalSection", &[section]);
    let address = v
        .api
        .register("kernel32.dll", Some("TryEnterCriticalSection"), None);
    let index = v.api.stub_index(address).unwrap();
    let sp = DATA + 4092;
    v.mem.write_u32(sp + 4, section).unwrap();
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
        1_000_000
    )
    .is_err());
    assert_eq!(v.cpu.regs[0], 1);
    assert_eq!(v.cpu.esp(), sp);
    assert_eq!(v.cpu.eip, address);
    assert_eq!(v.mem.read_u32(section + 8).unwrap(), 1);
    assert_eq!(v.mem.read_u32(section + 12).unwrap(), v.sched.current_tid());
}
