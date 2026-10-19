//! ETW provider lookup scopes and parameter carriers before typed binding.
use super::*;

#[test]
fn etw_win64_keeps_static_providers_and_registered_gateway_dynamic_lookup() {
    let (mut mem, mut cpu, mut rt) = startup();
    let text = STACK + 0x100;
    for dll in ["advapi32.dll", "kernel32.dll", "kernelbase.dll"] {
        rt.register_provider(dll);
        mem.write(text, format!("{dll}\0").as_bytes()).unwrap();
        let module = startup_call(&mut mem, &mut cpu, &mut rt, "LoadLibraryA", &[text]);
        for name in [
            "EventRegister",
            "EventUnregister",
            "EventEnabled",
            "EventProviderEnabled",
            "EventWrite",
            "EventWriteString",
            "EventWriteTransfer",
            "EventSetInformation",
        ] {
            assert!(wire::x64::runtime::import_supported(dll, name));
            assert!(!wire::x64::runtime::import_supported("ntdll.dll", name));
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
    }
}

#[test]
fn etw_win64_keeps_byte_and_u64_trace_carriers_dword_truncation_and_last_error() {
    let (mut mem, mut cpu, mut rt) = startup();
    let guid = STACK + 0x100;
    let output = STACK + 0x200;
    mem.write(guid, &[0x42; 16]).unwrap();
    startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut rt,
            "advapi32.dll",
            "EventRegister",
            &[guid, 1, 2, output],
        ),
        0
    );
    let handle = mem.read_int(output, 8).unwrap();
    for (name, args, ret, trace_args) in [
        (
            "EventProviderEnabled",
            vec![handle, 0x100000105, u64::MAX],
            0,
            "100000105, ffffffffffffffff]",
        ),
        (
            "EventWriteString",
            vec![handle, 0x100000105, u64::MAX, 1],
            0,
            "100000105, ffffffffffffffff, 1]",
        ),
        (
            "EventSetInformation",
            vec![handle, 0x100000001, 1, 0x100000004],
            50,
            ", 1, 1, 4]",
        ),
    ] {
        assert_eq!(
            startup_dll_call(&mut mem, &mut cpu, &mut rt, "advapi32.dll", name, &args,),
            ret
        );
        assert!(rt.trace.iter().last().unwrap().contains(trace_args));
    }
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
        123
    );
}
