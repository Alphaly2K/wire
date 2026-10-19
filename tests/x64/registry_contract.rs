//! Provider scope independent of the catalog, captured before typed binding.
use super::*;

#[test]
fn registry_win64_preserves_static_providers_registered_lookup_and_legacy_query_absence() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["advapi32.dll", "kernel32.dll", "kernelbase.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "RegCreateKeyA",
            "RegCreateKeyW",
            "RegCreateKeyExA",
            "RegCreateKeyExW",
            "RegOpenKeyA",
            "RegOpenKeyW",
            "RegOpenKeyExA",
            "RegOpenKeyExW",
            "RegQueryValueExA",
            "RegQueryValueExW",
            "RegSetValueExA",
            "RegSetValueExW",
            "RegDeleteValueA",
            "RegDeleteValueW",
            "RegCloseKey",
        ] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            assert!(!wire::x64::runtime::import_supported("user32.dll", name));
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            let initial = startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "GetProcAddress",
                &[module, text],
            );
            assert_eq!(initial != 0, dll == "advapi32.dll", "{dll}!{name}");
            let address = rt.register(dll, name);
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text]
                ),
                address
            );
        }
        for name in ["RegQueryValueA", "RegQueryValueW"] {
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
        }
        let out = STACK + 0x300;
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                dll,
                "RegOpenKeyExA",
                &[0x80000001, 0, 0x100000000, 0x100000003, out]
            ),
            0
        );
        assert!(rt.trace.iter().last().unwrap().contains(", 0, 3, "));
        let handle = mem.read_int(out, 8).unwrap();
        assert!(handle > u32::MAX as u64);
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut rt, dll, "RegCloseKey", &[handle]),
            0
        );
    }
}
