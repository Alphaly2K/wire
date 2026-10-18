//! Original message widths and output/metadata commits through Win64 gateways.
use super::*;
use wire::x64::process::Process64;

fn call(p: &mut Process64, name: &str, args: &[u64]) -> u64 {
    startup_dll_call(
        &mut p.memory,
        &mut p.cpu,
        &mut p.runtime,
        "user32.dll",
        name,
        args,
    )
}

#[test]
fn message_contract_win64_keeps_pointer_payload_and_failed_output_metadata() {
    let (mut p, _) = super::window_queries_contract::window();
    let out = BASE + 0x1400;
    let extra = 0xfedcba9876543210;
    assert_eq!(call(&mut p, "SetMessageExtraInfo", &[extra]), 0);
    let wparam = 0xabcdef0100000007;
    let lparam = 0xfedcba9800000009;
    assert_eq!(
        call(
            &mut p,
            "PostThreadMessageW",
            &[0xffff000000000001, 0xaaaa000000008001, wparam, lparam]
        ),
        1
    );
    let rsp = p.cpu.regs[4];
    p.memory.write_int(rsp, 8, BASE).unwrap();
    p.memory.write_int(rsp + 40, 8, 1).unwrap();
    p.cpu.regs[1] = 0x30000;
    p.cpu.regs[2] = 0;
    p.cpu.regs[8] = 0;
    p.cpu.regs[9] = 0;
    let before = p.cpu.clone();
    let calls = p.runtime.api_calls;
    assert!(p
        .runtime
        .dispatch("user32.dll", "PeekMessageW", &mut p.cpu, &mut p.memory)
        .is_err());
    assert_eq!(p.cpu, before);
    assert_eq!(p.runtime.api_calls, calls);
    assert_eq!(call(&mut p, "GetMessageExtraInfo", &[]), extra);
    assert_eq!(
        call(
            &mut p,
            "PeekMessageA",
            &[out, u64::MAX, 0x8001, 0x8001, 0xffff000000000001]
        ),
        1
    );
    assert_eq!(p.memory.read_int(out + 8, 4).unwrap(), 0x8001);
    assert_eq!(p.memory.read_int(out + 16, 8).unwrap(), wparam);
    assert_eq!(p.memory.read_int(out + 24, 8).unwrap(), lparam);
    assert_eq!(call(&mut p, "GetMessageExtraInfo", &[]), 0);
    assert_eq!(call(&mut p, "PeekMessageW", &[out, 0, 0, 0, 1]), 0);
}

#[test]
fn message_contract_win64_keeps_absent_timers_post_rejection_and_quit_width() {
    let (mut p, _) = super::window_queries_contract::window();
    for name in [
        "SetTimer",
        "KillTimer",
        "WaitMessage",
        "MsgWaitForMultipleObjects",
        "MsgWaitForMultipleObjectsEx",
        "GetQueueStatus",
    ] {
        assert!(!wire::x64::runtime::import_supported("user32.dll", name));
    }
    assert_eq!(
        call(&mut p, "GetMessageW", &[0x30000, u64::MAX, 2, 1]),
        u32::MAX as u64
    );
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        87
    );
    assert_eq!(call(&mut p, "PostMessageW", &[0, 0xc, 1, 2]), 0);
    assert_eq!(
        startup_call(
            &mut p.memory,
            &mut p.cpu,
            &mut p.runtime,
            "GetLastError",
            &[]
        ),
        1159
    );
    assert_eq!(call(&mut p, "PostQuitMessage", &[0xabcd000012345678]), 0);
    let out = BASE + 0x1400;
    assert_eq!(
        call(&mut p, "GetMessageW", &[out, u64::MAX, 0x8000, 0x8000]),
        0
    );
    assert_eq!(p.memory.read_int(out + 8, 4).unwrap(), 0x12);
    assert_eq!(p.memory.read_int(out + 16, 8).unwrap(), 0x12345678);
}
