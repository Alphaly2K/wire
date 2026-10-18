//! Independent class lookup, implicit controls and failed-registration boundaries.
use super::*;

#[test]
fn classes_contract_pe32_keeps_builtin_lookup_and_provider_scope() {
    let mut v = Vm::new();
    v.wide(DATA, "Button");
    v.k("SetLastError", &[77]);
    assert_eq!(
        v.call("user32.dll", "GetClassInfoExW", &[1, DATA, DATA + 512]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 48);
    assert_eq!(v.mem.read_u32(DATA + 516).unwrap(), 0x4000);
    assert_eq!(v.state.user32.classes.len(), 1);
    assert_eq!(v.state.last_error, 77);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 16);
    v.mem.write_u32(DATA + 512, 0xa5a5a5a5).unwrap();
    assert_eq!(
        v.call("kernel32.dll", "GetClassInfoExW", &[1, DATA, DATA + 512]),
        0
    );
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 0xa5a5a5a5);
}

#[test]
fn classes_contract_pe32_keeps_menu_fault_atom_commit_and_global_match() {
    let mut v = Vm::new();
    v.wide(DATA + 128, "WireGlobal");
    let fields = [
        48,
        0x4000,
        CODE,
        4,
        8,
        0x400000,
        2,
        3,
        4,
        0x30000,
        DATA + 128,
        5,
    ];
    for (i, value) in fields.into_iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, value).unwrap();
    }
    v.k("SetLastError", &[77]);
    assert!(v
        .try_call("user32.dll", "RegisterClassExW", &[DATA])
        .is_err());
    assert!(v.state.user32.classes.is_empty());
    assert_eq!(v.state.last_error, 77);
    v.mem.write_u32(DATA + 36, 42).unwrap();
    let atom = v.call("user32.dll", "RegisterClassExW", &[DATA]);
    assert_eq!(atom, 0xc000);
    assert_eq!(
        v.call(
            "user32.dll",
            "GetClassInfoW",
            &[0x99999999, atom, DATA + 512]
        ),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 512 + 8).unwrap(), 4);
    assert_eq!(v.mem.read_u32(DATA + 512 + 32).unwrap(), 42);
    assert_eq!(v.call("user32.dll", "RegisterClassExW", &[DATA]), 0);
    assert_eq!(v.state.last_error, 1410);
    assert!(v
        .try_call("user32.dll", "GetClassInfoW", &[0x400000, atom, 0x30000])
        .is_err());
    assert_eq!(v.state.user32.classes.len(), 1);
    assert_eq!(v.call("user32.dll", "UnregisterClassW", &[atom, 1]), 1);
    assert!(v.state.user32.classes.is_empty());
}

#[test]
fn classes_contract_pe32_keeps_field_errors_word_width_and_name_output() {
    let mut v = Vm::new();
    let proc = v.api.register("user32.dll", Some("DefWindowProcW"), None);
    let hwnd = test_window(&mut v, proc);
    assert_ne!(hwnd, 0);
    v.k("SetLastError", &[77]);
    assert_eq!(
        v.call(
            "user32.dll",
            "SetClassLongW",
            &[hwnd, (-14i32) as u32, 0x12345678]
        ),
        0
    );
    assert_eq!(
        v.call("user32.dll", "GetClassWord", &[hwnd, (-14i32) as u32]),
        0x12345678
    );
    assert_eq!(v.state.last_error, 77);
    assert_eq!(
        v.call(
            "user32.dll",
            "SetClassLongA",
            &[hwnd, (-20i32) as u32, 4097]
        ),
        0
    );
    assert_eq!(v.state.last_error, 87);
    assert_eq!(
        v.call("user32.dll", "SetClassLongW", &[hwnd, (-8i32) as u32, 1]),
        0
    );
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.call("user32.dll", "GetClassLongA", &[hwnd, 1]), 0);
    assert_eq!(v.state.last_error, 1413);
    v.mem.write_u32(DATA + 512, 0xa5a5a5a5).unwrap();
    assert_eq!(
        v.call("user32.dll", "GetClassNameW", &[hwnd, DATA + 512, 1]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 0xa5a50000);
    assert_eq!(
        v.call("user32.dll", "GetClassNameA", &[hwnd, DATA + 512, u32::MAX]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 512).unwrap(), 0xa5a50000);
    assert_eq!(v.call("user32.dll", "GetClassNameA", &[1, 0x30000, 10]), 0);
    assert_eq!(v.state.last_error, 1400);
}
