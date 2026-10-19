//! Historical import acceptance and dynamic discovery have distinct scopes.
use super::*;

#[test]
fn shell_contract_keeps_static_providers_and_requires_registered_gateway_for_lookup() {
    for name in ["CommandLineToArgvW", "SHGetKnownFolderPath"] {
        for dll in ["kernel32.dll", "kernelbase.dll", "shell32.dll", "ntdll.dll"] {
            assert_eq!(
                wire::x64::runtime::import_supported(dll, name),
                dll != "ntdll.dll"
            );
            if dll == "ntdll.dll" {
                continue;
            }
            let (mut mem, mut cpu, mut runtime) = startup();
            runtime.register_provider(dll);
            let input = STACK + 0x200;
            mem.write(input, &(dll.to_owned() + "\0").into_bytes())
                .unwrap();
            let module = startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "GetModuleHandleA",
                &[input],
            );
            assert_ne!(module, 0);
            mem.write(input, &(name.to_owned() + "\0").into_bytes())
                .unwrap();
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    "GetProcAddress",
                    &[module, input]
                ),
                0
            );
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
                127
            );
            let gateway = runtime.register(dll, name);
            assert_eq!(
                startup_call(
                    &mut mem,
                    &mut cpu,
                    &mut runtime,
                    "GetProcAddress",
                    &[module, input]
                ),
                gateway
            );
        }
    }
}
