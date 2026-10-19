//! Static token imports and dynamic lookup intentionally have different scopes.
use super::*;

#[test]
fn token_win64_keeps_static_providers_and_dynamic_lookup_requires_registered_gateway() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["advapi32.dll", "kernel32.dll", "kernelbase.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in ["OpenProcessToken", "OpenThreadToken", "GetTokenInformation"] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            assert!(!wire::x64::runtime::import_supported("user32.dll", name));
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            let address = startup_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                "GetProcAddress",
                &[module, text],
            );
            assert_eq!(address, 0, "unregistered {dll}!{name}");
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
                127
            );
            let registered = rt.register(dll, name);
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    "GetProcAddress",
                    &[module, text],
                ),
                registered
            );
        }
        for (handle, error) in [(u64::MAX - 1, 1008), (0xfffffffe, 6), (0, 6)] {
            assert_eq!(
                startup_dll_call(
                    &mut mem,
                    &mut cpu,
                    &mut rt,
                    dll,
                    "OpenThreadToken",
                    &[handle, 0x100000008, 0x100000001, 1],
                ),
                0
            );
            assert!(rt.trace.iter().last().unwrap().contains(&format!(
                "{dll}!OpenThreadToken args=[{handle:x}, 8, 1, 1] -> 0x0"
            )));
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
                error
            );
        }
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                dll,
                "OpenProcessToken",
                &[0, 8, 1],
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            6
        );
        assert_eq!(
            startup_dll_call(
                &mut mem,
                &mut cpu,
                &mut rt,
                dll,
                "GetTokenInformation",
                &[1, 20, 1, 4, 1],
            ),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            6
        );
    }
}
