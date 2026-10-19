//! Original diagnostic dialogs and window-station query policies.
use super::*;

#[test]
fn user_endpoints_keep_diagnostic_text_station_outputs_and_provider_abi() {
    let mut v = Vm::new();
    v.k("SetLastError", &[77]);
    assert_eq!(
        v.call("user32.dll", "GetProcessWindowStation", &[]),
        0x70070000
    );
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252);
    v.mem.write_u32(DATA, 0xa5a5a5a5).unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "GetUserObjectInformationW",
            &[0, 1, DATA, 4, 0x30000000]
        ),
        0
    );
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 232);
    assert_eq!(v.state.last_error, 77);
    v.mem.write_bytes(DATA, b"text\0").unwrap();
    v.mem.write_bytes(DATA + 0x100, b"caption\0").unwrap();
    assert_eq!(
        v.call(
            "user32.dll",
            "MessageBoxA",
            &[0, DATA, DATA + 0x100, u32::MAX]
        ),
        1
    );
    assert!(v
        .log
        .recent
        .back()
        .unwrap()
        .ends_with("SHOWN: [caption] text"));
    v.wide(DATA, "本文");
    v.wide(DATA + 0x100, "見出し");
    assert_eq!(
        v.call("user32.dll", "MessageBoxW", &[0, DATA, DATA + 0x100, 0]),
        1
    );
    assert!(v
        .log
        .recent
        .back()
        .unwrap()
        .ends_with("SHOWN: [見出し] 本文"));
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 236);
    assert_eq!(
        v.call("user32.dll", "MessageBoxW", &[0, 0x30000000, 0, 0]),
        1
    );
    assert!(v.log.recent.back().unwrap().ends_with("SHOWN: [] <fault>"));
    assert_eq!(v.state.last_error, 77);
    for name in [
        "GetProcessWindowStation",
        "GetUserObjectInformationW",
        "MessageBoxA",
        "MessageBoxW",
    ] {
        assert_eq!(v.call("kernel32.dll", name, &[0; 5]), 0);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252);
        assert_eq!(v.state.last_error, 120);
    }
    // The loaded MPR spec supplies four slots even though pointers are ignored.
    for name in ["WNetGetUniversalNameA", "WNetGetUniversalNameW"] {
        v.k("SetLastError", &[77]);
        assert_eq!(v.call("mpr.dll", name, &[0x30000000; 4]), 2250);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 236);
        assert_eq!(v.state.last_error, 77);
        assert!(
            wire::file_api::dispatch("mpr.dll", name, &[0; 16], &v.mem, &mut v.state).is_none()
        );
    }
    // These older public facades did not own the root gateway's MessageBox cases.
    for name in ["MessageBoxA", "MessageBoxW"] {
        assert!(wire::user32::dispatch(
            "user32.dll",
            name,
            &[0; 16],
            &mut v.cpu,
            &mut v.mem,
            &mut v.api,
            &mut v.state,
            &mut v.log,
            &mut v.sched,
            1_000_000
        )
        .is_none());
    }
}
