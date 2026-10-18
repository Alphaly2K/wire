//! Independent logical registered names and window property lifetime/ABI limits.
use super::*;

#[test]
fn user_objects_contract_pe32_keeps_shared_name_namespace_and_validation() {
    let mut v = Vm::new();
    v.k("SetLastError", &[77]);
    assert!(v
        .try_call("user32.dll", "RegisterClipboardFormatA", &[0x30000])
        .is_err());
    assert_eq!(v.state.last_error, 77);
    v.wide(DATA, "");
    assert_eq!(v.call("user32.dll", "RegisterWindowMessageW", &[DATA]), 0);
    assert_eq!(v.state.last_error, 87);
    v.wide(DATA, &"a".repeat(256));
    assert_eq!(v.call("user32.dll", "RegisterClipboardFormatW", &[DATA]), 0);
    assert_eq!(v.state.last_error, 87);
    v.wide(DATA, "K");
    v.mem.write_bytes(DATA + 512, b"k\0").unwrap();
    v.k("SetLastError", &[77]);
    assert_eq!(
        v.call("user32.dll", "RegisterClipboardFormatW", &[DATA]),
        0xc000
    );
    assert_eq!(
        v.call("user32.dll", "RegisterWindowMessageA", &[DATA + 512]),
        0xc000
    );
    assert_eq!(v.state.last_error, 77);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 8);
    assert_eq!(
        v.call("kernel32.dll", "RegisterWindowMessageA", &[DATA + 512]),
        0
    );
    assert_eq!(v.state.last_error, 120);
}

#[test]
fn user_objects_contract_pe32_keeps_atom_capacity_and_existing_lookup_when_full() {
    let mut v = Vm::new();
    for i in 0..0x4000 {
        v.mem
            .write_bytes(DATA, format!("name-{i}\0").as_bytes())
            .unwrap();
        assert_eq!(
            v.call("user32.dll", "RegisterWindowMessageA", &[DATA]),
            0xc000 + i
        );
    }
    v.mem.write_bytes(DATA, b"overflow\0").unwrap();
    assert_eq!(v.call("user32.dll", "RegisterClipboardFormatA", &[DATA]), 0);
    assert_eq!(v.state.last_error, 8);
    v.wide(DATA, "NAME-16383");
    assert_eq!(
        v.call("user32.dll", "RegisterClipboardFormatW", &[DATA]),
        0xffff
    );
    assert_eq!(v.state.last_error, 8);
}

#[test]
fn user_objects_contract_pe32_keeps_property_key_order_values_and_window_lifetime() {
    let mut v = Vm::new();
    let proc = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, proc);
    assert_ne!(hwnd, 0);
    v.k("SetLastError", &[77]);
    assert!(v.try_call("user32.dll", "GetPropA", &[1, 0x30000]).is_err());
    assert_eq!(v.state.last_error, 77);
    assert_eq!(v.call("user32.dll", "GetPropW", &[1, 0]), 0);
    assert_eq!(v.state.last_error, 1400);
    assert_eq!(v.call("user32.dll", "SetPropA", &[hwnd, 0, 1]), 0);
    assert_eq!(v.state.last_error, 87);
    v.mem.write_bytes(DATA + 512, b"123\0").unwrap();
    v.wide(DATA + 640, "123");
    v.wide(DATA + 768, "");
    v.k("SetLastError", &[77]);
    assert_eq!(v.call("user32.dll", "SetPropA", &[hwnd, 123, 42]), 1);
    assert_eq!(v.call("user32.dll", "SetPropW", &[hwnd, DATA + 640, 99]), 1);
    assert_eq!(v.call("user32.dll", "GetPropW", &[hwnd, 123]), 42);
    assert_eq!(v.call("user32.dll", "GetPropA", &[hwnd, DATA + 512]), 99);
    assert_eq!(v.call("user32.dll", "SetPropA", &[hwnd, 123, 0]), 1);
    assert_eq!(v.call("user32.dll", "RemovePropW", &[hwnd, 123]), 0);
    assert_eq!(v.call("user32.dll", "SetPropW", &[hwnd, DATA + 768, 7]), 1);
    assert_eq!(v.call("user32.dll", "RemovePropA", &[hwnd, DATA + 769]), 7);
    assert_eq!(v.state.last_error, 77);
    assert_eq!(v.call("user32.dll", "DestroyWindow", &[hwnd]), 1);
    assert_eq!(v.call("user32.dll", "GetPropW", &[hwnd, DATA + 640]), 0);
    assert_eq!(v.state.last_error, 1400);
}

#[test]
fn user_objects_contract_pe32_keeps_enumeration_abi_only_and_cleanup() {
    let mut v = Vm::new();
    for (name, args, cleanup) in [
        ("EnumPropsA", vec![1, 0x30000], 8),
        ("EnumPropsW", vec![1, 0x30000], 8),
        ("EnumPropsExA", vec![1, 0x30000, 42], 12),
        ("EnumPropsExW", vec![1, 0x30000, 42], 12),
    ] {
        assert_eq!(v.call("user32.dll", name, &args), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4 + cleanup);
    }
}
