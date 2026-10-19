//! Independent PE32 resource layouts and the legacy gateway commit order.
use super::*;

fn image(v: &mut Vm) -> (u32, u32) {
    let base = v.state.exe_base;
    v.mem.commit(base, 0x4000).unwrap();
    v.mem.write_u32(base + 0x3c, 0x80).unwrap();
    v.mem.write_u32(base + 0x80, 0x4550).unwrap();
    v.mem.write_u32(base + 0x108, 0x1000).unwrap();
    v.mem.write_u32(base + 0x10c, 0x1000).unwrap();
    let root = base + 0x1000;
    for (offset, value) in [(14, 1), (38, 1), (62, 2)] {
        v.mem.write_u16(root + offset, value).unwrap();
    }
    for (offset, value) in [
        (16, 6),
        (20, 0x80000018),
        (40, 1),
        (44, 0x80000030),
        (64, 0x411),
        (68, 80),
        (72, 0x409),
        (76, 96),
        (80, 0x2000),
        (84, 36),
        (96, 0x2100),
        (100, 40),
    ] {
        v.mem.write_u32(root + offset, value).unwrap();
    }
    v.mem.write_u16(base + 0x2000, 2).unwrap();
    v.wide(base + 0x2002, "日本");
    (base, root)
}

#[test]
fn resource_contract_pe32_keeps_default_image_language_fallback_and_named_keys() {
    let mut v = Vm::new();
    let (base, root) = image(&mut v);
    v.k("SetLastError", &[77]);
    for name in ["FindResourceA", "FindResourceW"] {
        assert_eq!(v.k(name, &[0, 1, 6]), root + 80);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 16);
    }
    for name in ["FindResourceExA", "FindResourceExW"] {
        assert_eq!(v.k(name, &[base, 6, 1, 0x409]), root + 96);
        assert_eq!(v.k(name, &[0, 6, 1, 0x777]), root + 80);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 20);
    }
    assert_eq!(v.state.last_error, 77);
    // Named entries are length-prefixed UTF-16; ASCII case folding is retained.
    v.mem.write_u32(root + 16, 0x80000100).unwrap();
    v.mem.write_u32(root + 40, 0x80000120).unwrap();
    v.mem.write_u16(root + 0x100, 4).unwrap();
    v.wide(root + 0x102, "Type");
    v.mem.write_u16(root + 0x120, 2).unwrap();
    v.wide(root + 0x122, "日本");
    v.wide(DATA, "type");
    v.wide(DATA + 32, "日本");
    assert_eq!(v.k("FindResourceW", &[0, DATA + 32, DATA]), root + 80);
    v.mem.write_bytes(DATA, b"TYPE\0").unwrap();
    v.mem
        .write_bytes(DATA + 32, &[0x93, 0xfa, 0x96, 0x7b, 0])
        .unwrap();
    assert_eq!(
        v.k("FindResourceExA", &[0, DATA, DATA + 32, 0x409]),
        root + 96
    );
    assert_eq!(v.state.last_error, 77);
    assert_eq!(v.k("FindResourceA", &[0, 1, 6]), 0);
    assert_eq!(v.state.last_error, 1814);
    let calls = v.log.call_count;
    assert!(v
        .try_call("kernel32.dll", "FindResourceExW", &[0, 0x30000, 1, 0])
        .unwrap_err()
        .starts_with("resource name: "));
    assert_eq!(v.log.call_count, calls);
    assert_eq!(v.state.last_error, 1814);
}

#[test]
fn resource_contract_pe32_keeps_string_lengths_cp932_and_return_slot_alias() {
    let mut v = Vm::new();
    let (base, _) = image(&mut v);
    v.k("SetLastError", &[77]);
    assert_eq!(
        v.call("user32.dll", "LoadStringW", &[0, 0x10000, DATA, 2]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0x65e5);
    assert_eq!(v.call("user32.dll", "LoadStringA", &[0, 0, DATA, 2]), 1);
    assert_eq!(v.mem.read_u16(DATA).unwrap(), 0x93); // Retains byte truncation inside DBCS.
    v.mem.write_u32(DATA, 0xa5a5a5a5).unwrap();
    for (name, count) in [
        ("LoadStringA", 0),
        ("LoadStringA", u32::MAX),
        ("LoadStringW", u32::MAX),
    ] {
        assert_eq!(v.call("user32.dll", name, &[0, 0, DATA, count]), 0);
        assert_eq!(v.mem.read_u32(DATA).unwrap(), 0xa5a5a5a5);
    }
    assert_eq!(v.call("user32.dll", "LoadStringW", &[0, 0, DATA, 0]), 2);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), base + 0x2002);
    assert_eq!(v.call("user32.dll", "LoadStringW", &[0, 1, DATA, 0]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), base + 0x2008); // Empty slot keeps its address.
    assert_eq!(v.call("user32.dll", "LoadStringW", &[0, 16, DATA, 0]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0);
    let sp = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.call("user32.dll", "LoadStringW", &[0, 0, sp, 0]), 2);
    assert_eq!(v.cpu.eip, base + 0x2002); // Output precedes reading the return frame.
    assert_eq!(v.cpu.esp(), sp + 20);
    assert_eq!(v.state.last_error, 77);
    assert!(v
        .log
        .recent
        .back()
        .unwrap()
        .ends_with(" ; resource string 0: 2 UTF-16 units"));
}

#[test]
fn resource_contract_pe32_keeps_partial_string_writes_before_fault() {
    let mut v = Vm::new();
    image(&mut v);
    v.k("SetLastError", &[77]);
    let calls = v.log.call_count;
    assert!(v
        .try_call("user32.dll", "LoadStringW", &[0, 0, DATA + 0xfffe, 3])
        .is_err());
    assert_eq!(v.mem.read_u16(DATA + 0xfffe).unwrap(), 0x65e5);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256);
    assert_eq!(v.log.call_count, calls);
    assert_eq!(v.state.last_error, 77);
    assert!(v
        .try_call("user32.dll", "LoadStringA", &[0, 0, DATA + 0xffff, 2])
        .is_err());
    assert_eq!(v.mem.read_u8(DATA + 0xffff).unwrap(), 0x93);
    assert_eq!(v.log.call_count, calls);
    assert_eq!(v.state.last_error, 77);
    assert!(v
        .try_call("user32.dll", "LoadStringW", &[0, 0, 0x30000, 0])
        .is_err());
    assert_eq!(v.log.call_count, calls);
}

#[test]
fn resource_contract_pe32_keeps_data_handles_errors_and_wrong_provider_abi() {
    let mut v = Vm::new();
    let (base, root) = image(&mut v);
    v.k("SetLastError", &[77]);
    assert_eq!(v.k("LoadResource", &[0, root + 80]), base + 0x2000);
    assert_eq!(v.k("SizeofResource", &[0, root + 80]), 36);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 12);
    for name in ["LoadResource", "SizeofResource"] {
        assert_eq!(v.k(name, &[0, 0x30000]), 0);
        assert_eq!(v.state.last_error, 77);
    }
    assert_eq!(v.k("LockResource", &[0xdeadbeef]), 0xdeadbeef);
    assert_eq!(v.k("FreeResource", &[0xdeadbeef]), 0);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 8);
    assert_eq!(v.state.last_error, 77);
    v.mem.write_u32(base + 0x80, 0).unwrap();
    assert_eq!(v.k("FindResourceW", &[0, 1, 6]), 0);
    assert_eq!(v.state.last_error, 1814);
    for (dll, name) in [
        ("kernelbase.dll", "FindResourceW"),
        ("user32.dll", "LoadResource"),
        ("kernel32.dll", "LoadStringW"),
    ] {
        assert_eq!(v.call(dll, name, &[0, 0x30000, 0x30000, 0]), 0);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 4);
        assert_eq!(v.state.last_error, 120);
        assert!(v
            .log
            .recent
            .back()
            .unwrap()
            .contains("unknown stdcall arity"));
    }
}
