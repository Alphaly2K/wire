//! Four PE32 Variant helpers remain absent from the Win64 service set.
use super::*;

#[test]
fn variant_contract_keeps_win64_absent_imports_and_lookup() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    mem.write(text, b"oleaut32.dll\0").unwrap();
    let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
    for name in [
        "VariantInit",
        "VariantClear",
        "VariantCopy",
        "VariantChangeType",
    ] {
        for dll in ["oleaut32.dll", "kernel32.dll", "ole32.dll", "combase.dll"] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
        }
        mem.write(text, format!("{name}\0").as_bytes()).unwrap();
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "GetProcAddress",
                &[module, text]
            ),
            0
        );
    }
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetProcAddress", &[module, 9]),
        0
    );
}
