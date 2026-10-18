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
