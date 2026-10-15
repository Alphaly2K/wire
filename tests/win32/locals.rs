//! Historical PE32 local-slot behavior, independent of catalog metadata.
use super::*;

#[test]
fn locals_keep_pe32_permissive_slots_error_rules_and_global_free() {
    let mut v = Vm::new();
    assert_eq!(v.k("TlsAlloc", &[]), 1);
    assert_eq!(v.k("FlsAlloc", &[CODE]), 1);
    let slot = 0xfedcba98;
    v.k("SetLastError", &[0x55]);
    assert_eq!(v.k("TlsSetValue", &[slot, 0x87654321]), 1);
    assert_eq!(v.k("GetLastError", &[]), 0x55);
    assert_eq!(v.k("TlsGetValue", &[slot]), 0x87654321);
    assert_eq!(v.k("GetLastError", &[]), 0);
    v.k("SetLastError", &[0x55]);
    assert_eq!(v.k("FlsSetValue", &[slot, 0x12345678]), 1);
    assert_eq!(v.k("FlsGetValue", &[slot]), 0x12345678);
    assert_eq!(v.k("GetLastError", &[]), 0x55);
    let tid = v.sched.current_tid();
    v.sched.threads[v.sched.current].tid = tid + 1;
    assert_eq!(v.k("TlsGetValue", &[slot]), 0);
    assert_eq!(v.k("FlsGetValue", &[slot]), 0);
    assert_eq!(v.k("TlsSetValue", &[slot, 7]), 1);
    assert_eq!(v.k("FlsSetValue", &[slot, 8]), 1);
    assert_eq!(v.k("TlsFree", &[slot]), 1);
    assert_eq!(v.k("FlsFree", &[slot]), 1);
    v.sched.threads[v.sched.current].tid = tid;
    assert_eq!(v.k("TlsGetValue", &[slot]), 0);
    assert_eq!(v.k("FlsGetValue", &[slot]), 0);
    assert_eq!(v.k("TlsFree", &[slot]), 1);
    assert_eq!(v.k("FlsFree", &[slot]), 1);
    assert_eq!(v.k("TlsAlloc", &[]), 2);
    assert_eq!(v.k("FlsAlloc", &[0]), 2);
}

#[test]
fn locals_keep_pe32_allocation_commit_before_bad_return_slot() {
    let mut v = Vm::new();
    let address = v.api.register("kernel32.dll", Some("TlsAlloc"), None);
    let index = v.api.stub_index(address).unwrap();
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_NOACCESS)
        .unwrap();
    v.cpu.set_esp(DATA);
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
    assert_eq!(v.cpu.esp(), DATA);
    assert_eq!(v.cpu.eip, address);
    assert_eq!(v.k("TlsAlloc", &[]), 2);
}
