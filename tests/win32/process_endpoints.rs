//! Independent legacy kernel endpoint behavior, including return-frame faults.
use super::*;

#[test]
fn kernel_endpoints_keep_capacity_hint_package_outputs_and_provider_cleanup() {
    let mut v = Vm::new();
    v.k("SetLastError", &[77]);
    let handles = v.state.handles.len();
    assert_eq!(v.k("SetHandleCount", &[u32::MAX]), u32::MAX);
    assert_eq!(v.state.handles.len(), handles);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 248);
    assert!(v
        .log
        .recent
        .back()
        .unwrap()
        .ends_with("automatic handle table capacity"));
    v.mem.write_u32(DATA, 0xa5a5a5a5).unwrap();
    assert_eq!(v.k("GetCurrentPackageId", &[DATA, 0x30000000]), 0x3d54);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 244);
    assert_eq!(v.state.last_error, 77);
    assert!(v.log.recent.back().unwrap().ends_with("not a packaged app"));
    for name in ["SetHandleCount", "GetCurrentPackageId", "TerminateProcess"] {
        assert_eq!(v.call("kernelbase.dll", name, &[0; 2]), 0);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252);
        assert!(!v.cpu.halted);
        assert_eq!(v.state.last_error, 120);
    }
}

#[test]
fn terminate_process_keeps_handle_validation_and_halts_before_return_fault() {
    for use_handle in [false, true] {
        let mut v = Vm::new();
        let event = v.k("CreateEventW", &[0, 0, 0, 0]);
        assert_eq!(v.k("TerminateProcess", &[event, 19]), 0);
        assert_eq!(v.state.last_error, 6);
        assert!(!v.cpu.halted);
        let process = if use_handle {
            v.k("OpenProcess", &[0, 0, 4242])
        } else {
            u32::MAX
        };
        v.k("SetLastError", &[77]);
        assert_eq!(v.k("TerminateProcess", &[process, 0x87654321]), 1);
        assert!(v.cpu.halted);
        assert_eq!(v.cpu.exit_code, Some(0x87654321));
        assert_eq!(v.cpu.eip, RET);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 244);
        assert_eq!(v.state.last_error, 77);
    }
    let mut v = Vm::new();
    let address = v
        .api
        .register("kernel32.dll", Some("TerminateProcess"), None);
    let index = v.api.stub_index(address).unwrap();
    let sp = DATA + 4096 - 4;
    v.mem.write_u32(sp, RET).unwrap();
    v.mem.write_u32(sp + 4, u32::MAX).unwrap();
    v.mem.write_u32(sp + 8, 23).unwrap();
    v.mem
        .virtual_protect(DATA, 4096, wire::mem::PAGE_NOACCESS)
        .unwrap();
    v.cpu.eip = address;
    v.cpu.set_esp(sp);
    let error = api::dispatch(
        index,
        &mut v.cpu,
        &mut v.mem,
        &mut v.api,
        &mut v.state,
        &mut v.log,
        &mut v.sched,
        1_000_000,
    )
    .unwrap_err();
    assert!(error.contains("stub stack fault"), "{error}");
    assert!(v.cpu.halted);
    assert_eq!(v.cpu.exit_code, Some(23));
    assert_eq!(v.cpu.regs[0], 1);
    assert_eq!(v.cpu.eip, address);
    assert_eq!(v.cpu.esp(), sp);
}
