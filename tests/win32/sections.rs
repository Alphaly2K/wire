//! File-section ABI baselines independent of the catalog being migrated.
use super::*;

#[test]
fn section_contract_baseline_keeps_pe32_abi_only_cleanup_and_outputs() {
    let mut v = Vm::new();
    for (name, slots) in [
        ("CreateFileMappingA", 6),
        ("CreateFileMappingW", 6),
        ("OpenFileMappingA", 3),
        ("OpenFileMappingW", 3),
        ("MapViewOfFile", 5),
        ("MapViewOfFileEx", 6),
        ("UnmapViewOfFile", 1),
        ("FlushViewOfFile", 2),
    ] {
        v.mem.write_bytes(DATA, &[0xa5; 32]).unwrap();
        let handles = v.state.handles.len();
        assert_eq!(v.k(name, &[DATA; 6][..slots]), 0);
        assert_eq!(
            v.cpu.esp(),
            STACK_BASE + STACK_SIZE - 256 + 4 + slots as u32 * 4
        );
        assert_eq!(v.cpu.eip, RET);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
        assert_eq!(v.state.handles.len(), handles);
        assert_eq!(v.call("user32.dll", name, &[DATA; 6][..slots]), 0);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
    }
}

#[test]
fn section_contract_baseline_keeps_pe32_error_and_eax_before_return_frame_fault() {
    let mut v = Vm::new();
    let address = v
        .api
        .register("kernel32.dll", Some("FlushViewOfFile"), None);
    let index = v.api.stub_index(address).unwrap();
    let sp = DATA + 4096 - 4;
    v.mem.write_u32(sp, RET).unwrap();
    v.mem.write_u32(sp + 4, 0).unwrap();
    v.mem.write_u32(sp + 8, 0).unwrap();
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_NOACCESS)
        .unwrap();
    v.state.last_error = 0x55;
    v.cpu.regs[0] = 0x1234;
    v.cpu.eip = address;
    v.cpu.set_esp(sp);
    let handles = v.state.handles.len();
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
    .unwrap_err()
    .contains("stub stack fault"));
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.cpu.regs[0], 0);
    assert_eq!(v.cpu.eip, address);
    assert_eq!(v.cpu.esp(), sp);
    assert_eq!(v.state.handles.len(), handles);
}
