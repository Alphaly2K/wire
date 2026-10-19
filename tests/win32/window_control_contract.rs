//! Legacy window control validation and state commits before guest callbacks.
use super::*;

#[test]
fn window_control_contract_pe32_keeps_show_modes_and_destroy_thread_checks() {
    let mut v = Vm::new();
    let procedure = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, procedure);
    assert_eq!(v.call("user32.dll", "ShowWindow", &[hwnd, 1]), 0);
    assert_eq!(v.call("user32.dll", "ShowWindow", &[hwnd, 0]), 1);
    v.state.user32.windows.get_mut(&hwnd).unwrap().tid += 1;
    // PE32 treats every nonzero command as shown, without an owner check.
    assert_eq!(v.call("user32.dll", "ShowWindow", &[hwnd, 2]), 0);
    assert_ne!(v.state.user32.windows[&hwnd].style & 0x10000000, 0);
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 0);
    assert_eq!(v.state.last_error, 5);
    assert!(v.state.user32.windows.contains_key(&hwnd));
    v.state.user32.windows.get_mut(&hwnd).unwrap().tid -= 1;
    assert_eq!(v.call("user32.dll", "SetParent", &[hwnd, hwnd]), 0);
    assert_eq!(v.state.last_error, 87);
    assert_eq!(v.state.user32.windows[&hwnd].parent, 0);
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 1);
}

#[test]
fn window_control_contract_pe32_keeps_show_and_destroy_committed_on_callback_fault() {
    let mut v = Vm::new();
    let procedure = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, procedure);
    assert_eq!(v.call("user32.dll", "ShowWindow", &[hwnd, 1]), 0);
    v.mem.write_bytes(CODE, &[0x0f, 0x0b]).unwrap();
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-4i32) as u32, CODE],
    );
    assert!(v.try_call("user32.dll", "ShowWindow", &[hwnd, 0]).is_err());
    assert_eq!(v.state.user32.windows[&hwnd].style & 0x10000000, 0);
    assert!(v.try_call("user32.dll", "DestroyWindow", &[hwnd]).is_err());
    assert!(v.state.user32.windows.contains_key(&hwnd));
    // Destruction was marked before the failed callback; a retry does not
    // invoke the faulting procedure again or remove the retained window.
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 0);
    assert!(v.state.user32.windows.contains_key(&hwnd));
}

#[test]
fn default_windowpos_reloads_size_target_after_move_reentry() {
    let mut v = Vm::new();
    let default = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let set = v.api.register("user32.dll", Some("SetWindowLongW"), None);
    let hwnd = test_window(&mut v, default);
    let size_proc = CODE + 0x100;
    // WM_MOVE replaces the procedure and minimizes the window before WM_SIZE.
    let mut moved = Vec::new();
    for (index, value) in [((-4i32) as u32, size_proc), ((-16i32) as u32, 0x20000000)] {
        for value in [value, index, hwnd] {
            moved.push(0x68);
            moved.extend(value.to_le_bytes());
        }
        moved.push(0xb8);
        moved.extend(set.to_le_bytes());
        moved.extend([0xff, 0xd0]);
    }
    moved.extend([0x31, 0xc0, 0xc2, 16, 0]);
    v.mem.write_bytes(CODE, &moved).unwrap();
    let mut sized = vec![0x8b, 0x44, 0x24, 12, 0xa3];
    sized.extend((DATA + 0x100).to_le_bytes());
    sized.extend([0x8b, 0x44, 0x24, 16, 0xa3]);
    sized.extend((DATA + 0x104).to_le_bytes());
    sized.extend([0xff, 0x05]);
    sized.extend((DATA + 0x108).to_le_bytes());
    sized.extend([0x31, 0xc0, 0xc2, 16, 0]);
    v.mem.write_bytes(size_proc, &sized).unwrap();
    v.call(
        "user32.dll",
        "SetWindowLongW",
        &[hwnd, (-4i32) as u32, CODE],
    );
    v.mem.write_bytes(DATA, &[0; 28]).unwrap();
    v.mem.write_u32(DATA, hwnd).unwrap();
    v.mem.write_u32(DATA + 0x100, u32::MAX).unwrap();
    v.mem.write_u32(DATA + 0x104, u32::MAX).unwrap();
    assert_eq!(
        v.call("user32.dll", "DefWindowProcW", &[hwnd, 0x47, 0, DATA]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x108).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 1); // SIZE_MINIMIZED
    assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 0);
    assert_eq!(v.state.user32.windows[&hwnd].wndproc, size_proc);
}
