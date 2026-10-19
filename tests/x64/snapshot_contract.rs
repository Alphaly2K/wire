//! Legacy static and dynamic Toolhelp providers, before typed binding.
use super::*;

#[test]
fn snapshot_win64_keeps_kernel_providers_dynamic_lookup_and_dword_creation_flags() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["kernel32.dll", "kernelbase.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "CreateToolhelp32Snapshot",
            "Process32First",
            "Process32Next",
            "Process32FirstW",
            "Process32NextW",
            "Thread32First",
            "Thread32Next",
        ] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            assert!(!wire::x64::runtime::import_supported("user32.dll", name));
            mem.write(text, format!("{name}\0").as_bytes()).unwrap();
            assert_ne!(
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
        assert!(!wire::x64::runtime::import_supported(
            dll,
            "Process32FirstA"
        ));
        mem.write(text, b"Process32FirstA\0").unwrap();
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
        let handle = startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            dll,
            "CreateToolhelp32Snapshot",
            &[0x100000006, u64::MAX],
        );
        assert_ne!(handle, u64::MAX);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "CloseHandle", &[handle]),
            1
        );
    }
}
