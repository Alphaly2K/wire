//! These resource gateways have no legacy Win64 implementation or lookup.
use super::*;

#[test]
fn resource_contract_win64_keeps_imports_and_fresh_provider_lookup_absent() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll", "user32.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "LoadStringA",
            "LoadStringW",
            "FindResourceA",
            "FindResourceW",
            "FindResourceExA",
            "FindResourceExW",
            "LoadResource",
            "SizeofResource",
            "LockResource",
            "FreeResource",
        ] {
            assert!(!wire::x64::runtime::import_supported(dll, name));
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
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
                127
            );
        }
    }
}
