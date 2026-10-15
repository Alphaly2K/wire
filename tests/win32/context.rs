//! Execution-context boundaries fixed independently of catalog declarations.
use super::*;

#[test]
fn context_keeps_pe32_thread_identity_pseudohandles_and_teb_last_error() {
    let mut v = Vm::new();
    let error = 0x87654321;
    v.mem.write_u32(TEB_ADDR + 0x34, error).unwrap();
    v.state.last_error = 0x55;
    v.sched.threads[v.sched.current].tid = 0x4567;
    let tid = v.sched.current_tid();
    for (name, expected) in [
        ("GetCurrentProcessId", 4242),
        ("GetCurrentThreadId", tid),
        ("GetCurrentProcess", u32::MAX),
        ("GetCurrentThread", u32::MAX - 1),
        ("IsDebuggerPresent", 0),
        ("GetLastError", error),
    ] {
        assert_eq!(v.k(name, &[]), expected);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252);
        assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), error);
        assert_eq!(v.state.last_error, error);
        assert!(!api::native_export("user32.dll", name));
    }
    assert_eq!(v.k("SetLastError", &[0xfedcba98]), 0);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 248);
    assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), 0xfedcba98);
    assert_eq!(v.k("GetLastError", &[]), 0xfedcba98);
}

#[test]
fn context_keeps_pe32_error_and_eax_commit_before_return_slot_fault() {
    let mut v = Vm::new();
    let address = v.api.register("kernel32.dll", Some("SetLastError"), None);
    let index = v.api.stub_index(address).unwrap();
    let sp = DATA + 4092;
    v.mem.write_u32(sp + 4, 0xabcdef12).unwrap();
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_NOACCESS)
        .unwrap();
    v.mem.write_u32(TEB_ADDR + 0x34, 0x55).unwrap();
    v.cpu.set_esp(sp);
    v.cpu.eip = address;
    v.cpu.regs[0] = 0x1234;
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
    assert_eq!(v.cpu.regs[0], 0);
    assert_eq!(v.cpu.eip, address);
    assert_eq!(v.cpu.esp(), sp);
    assert_eq!(v.state.last_error, 0xabcdef12);
    assert_eq!(v.mem.read_u32(TEB_ADDR + 0x34).unwrap(), 0x55);
}
