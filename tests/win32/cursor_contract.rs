//! Cursor owner behavior and the PE32 ABI-only pointer/clip facilities.
use super::*;

#[test]
fn cursor_contract_pe32_keeps_unimplemented_clip_and_pointer_queries() {
    let mut v = Vm::new();
    for (name, args) in [
        ("ClipCursor", vec![DATA]),
        ("GetClipCursor", vec![DATA]),
        ("EnableMouseInPointer", vec![1]),
        ("IsMouseInPointerEnabled", vec![]),
        ("TrackMouseEvent", vec![DATA]),
    ] {
        v.mem.write_bytes(DATA, &[0xa5; 32]).unwrap();
        v.k("SetLastError", &[77]);
        assert_eq!(v.call("user32.dll", name, &args), 0, "{name}");
        assert_eq!(v.state.last_error, 120, "{name}");
        assert_eq!(
            v.cpu.esp(),
            STACK_BASE + STACK_SIZE - 256 + 4 * (args.len() as u32 + 1),
            "{name}"
        );
        let mut bytes = [0; 32];
        v.mem.read_bytes(DATA, &mut bytes).unwrap();
        assert_eq!(bytes, [0xa5; 32]);
    }
}

#[test]
fn cursor_contract_pe32_keeps_arrow_full_signed_position_and_return_slot_order() {
    let mut v = Vm::new();
    let arrow = v.call("user32.dll", "LoadCursorA", &[0, 32512]);
    assert_eq!(arrow, 0x42007f00);
    assert_eq!(v.call("user32.dll", "SetCursor", &[arrow]), 0);
    assert_eq!(v.call("user32.dll", "GetCursor", &[]), arrow);
    assert_eq!(v.call("user32.dll", "SetCursor", &[arrow + 1]), 0);
    assert_eq!(v.state.last_error, 6);
    assert_eq!(v.call("user32.dll", "GetCursor", &[]), arrow);
    assert_eq!(
        v.call(
            "user32.dll",
            "SetCursorPos",
            &[i32::MAX as u32, i32::MIN as u32]
        ),
        1
    );
    v.k("SetLastError", &[77]);
    let slot = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.call("user32.dll", "GetCursorPos", &[slot]), 1);
    assert_eq!(v.cpu.eip, i32::MAX as u32);
    assert_eq!(v.mem.read_u32(slot + 4).unwrap(), i32::MIN as u32);
    assert_eq!(v.state.last_error, 77);
    assert_eq!(v.call("user32.dll", "ShowCursor", &[0]), u32::MAX);
    assert_eq!(v.call("user32.dll", "ShowCursor", &[1]), 0);
}

#[test]
fn cursor_contract_pe32_position_fault_preserves_accessible_prefix() {
    let mut v = Vm::new();
    v.call("user32.dll", "SetCursorPos", &[11, 22]);
    let output = DATA + 0xfffc;
    v.mem.write_bytes(output, &[0xa5; 4]).unwrap();
    assert!(v.try_call("user32.dll", "GetCursorPos", &[output]).is_err());
    assert_eq!(v.mem.read_u32(output).unwrap(), 0xa5a5a5a5);
}
