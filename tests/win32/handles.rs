//! Handle gateway baselines; expectations do not depend on catalog declarations.
use super::*;

#[test]
fn handle_contract_baseline_keeps_pe32_null_stdio_and_abi_only_outputs() {
    let mut v = Vm::new();
    for selector in [0xfffffff6, 0xfffffff5, 0xfffffff4, 0] {
        v.k("SetLastError", &[0x55]);
        assert_eq!(v.k("GetStdHandle", &[selector]), 0);
        assert_eq!(v.state.last_error, 0x55);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 8);
    }
    for (name, slots) in [
        ("SetStdHandle", 2),
        ("DuplicateHandle", 7),
        ("GetHandleInformation", 2),
        ("SetHandleInformation", 3),
    ] {
        v.mem.write_bytes(DATA, &[0xa5; 32]).unwrap();
        let handles = v.state.handles.len();
        assert_eq!(v.k(name, &[DATA; 7][..slots]), 0);
        assert_eq!(
            v.cpu.esp(),
            STACK_BASE + STACK_SIZE - 256 + 4 + slots as u32 * 4
        );
        assert_eq!(v.cpu.eip, RET);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
        assert_eq!(v.state.handles.len(), handles);
    }
    for name in [
        "GetStdHandle",
        "SetStdHandle",
        "CloseHandle",
        "DuplicateHandle",
        "GetHandleInformation",
        "SetHandleInformation",
    ] {
        assert_eq!(v.call("kernelbase.dll", name, &[0; 7]), 0);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
    }
}

#[test]
fn handle_contract_baseline_keeps_pe32_close_before_return_frame_fault() {
    let mut v = Vm::new();
    let event = v.k("CreateEventA", &[0, 0, 0, 0]);
    let address = v.api.register("kernel32.dll", Some("CloseHandle"), None);
    let index = v.api.stub_index(address).unwrap();
    let sp = DATA + 4096 - 4;
    v.mem.write_u32(sp, RET).unwrap();
    v.mem.write_u32(sp + 4, event).unwrap();
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_NOACCESS)
        .unwrap();
    v.cpu.eip = address;
    v.cpu.set_esp(sp);
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
    assert!(!v.state.handles.contains_key(&event));
    assert_eq!(v.cpu.regs[0], 1);
    assert_eq!(v.cpu.eip, address);
    assert_eq!(v.cpu.esp(), sp);
}
