//! Original queue/fault ordering, independent of the future typed adapter.
use super::*;

#[test]
fn message_contract_pe32_failed_output_consumes_notification_but_keeps_message() {
    let mut v = Vm::new();
    let tid = v.sched.current_tid();
    assert_eq!(
        v.call("user32.dll", "PostThreadMessageW", &[tid, 0x8001, 7, 9]),
        1
    );
    assert_eq!(v.call("user32.dll", "WaitMessage", &[]), 1);
    assert_eq!(
        v.call("user32.dll", "MsgWaitForMultipleObjects", &[0, 0, 0, 0, 8]),
        0
    );
    assert!(v
        .try_call("user32.dll", "PeekMessageW", &[0x30000, 0, 0, 0, 1])
        .is_err());
    // Queue-change bits are cleared before the checked MSG write in PE32.
    assert_eq!(
        v.call("user32.dll", "MsgWaitForMultipleObjects", &[0, 0, 0, 0, 8]),
        0x102
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjectsEx",
            &[0, 0, 0, 8, 4]
        ),
        0
    );
    assert_eq!(
        v.call("user32.dll", "GetMessageA", &[DATA, u32::MAX, 0, 0]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0);
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 0x8001);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 7);
    assert_eq!(v.mem.read_u32(DATA + 12).unwrap(), 9);
    assert_eq!(v.call("user32.dll", "PeekMessageW", &[DATA, 0, 0, 0, 1]), 0);
}

#[test]
fn message_contract_pe32_keeps_filter_precedence_and_message_wait_validation() {
    let mut v = Vm::new();
    // The legacy PE32 error selection also checks the HWND_MESSAGE filter
    // after an invalid range; preserve this architecture-specific result.
    assert_eq!(
        v.call("user32.dll", "GetMessageW", &[0x30000, u32::MAX, 2, 1]),
        u32::MAX
    );
    assert_eq!(v.state.last_error, 1400);
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjectsEx",
            &[1, 0x30000, 0, 8, 2]
        ),
        u32::MAX
    );
    assert_eq!(v.state.last_error, 87);
    assert_eq!(
        v.call(
            "user32.dll",
            "MsgWaitForMultipleObjectsEx",
            &[0, 0, 0, 0x800, 0]
        ),
        u32::MAX
    );
    assert_eq!(v.state.last_error, 87);
    // PE32 permits these posted messages; Win64 rejects this sync-only ID.
    assert_eq!(v.call("user32.dll", "PostMessageW", &[0, 0xc, 1, 2]), 1);
    assert_eq!(v.call("user32.dll", "PostQuitMessage", &[17]), 1);
    assert_eq!(
        v.call("user32.dll", "GetMessageW", &[DATA, 0, 0x8000, 0x8000]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 0x12);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 17);
}
