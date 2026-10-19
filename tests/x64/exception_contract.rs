//! Exception gateway policy and resource ordering, independent of catalog data.
use super::*;

#[test]
fn exception_contract_preserves_provider_scope_and_full_width_filter_and_handler_tokens() {
    for (name, native) in [
        ("AddVectoredExceptionHandler", false),
        ("RemoveVectoredExceptionHandler", false),
        ("RaiseException", false),
        ("SetUnhandledExceptionFilter", false),
        ("RtlUnwindEx", true),
        ("RtlAddFunctionTable", true),
        ("RtlDeleteFunctionTable", true),
        ("RtlInstallFunctionTableCallback", true),
        ("RtlLookupFunctionEntry", true),
    ] {
        for dll in ["kernel32.dll", "kernelbase.dll", "ntdll.dll", "shell32.dll"] {
            assert_eq!(
                wire::x64::runtime::import_supported(dll, name),
                dll == "kernel32.dll" || dll == "kernelbase.dll" || (native && dll == "ntdll.dll"),
                "{dll}!{name}"
            );
        }
    }
    assert!(!wire::x64::runtime::import_supported(
        "kernel32.dll",
        "RtlUnwind"
    ));
    assert!(!wire::x64::runtime::import_supported(
        "kernel32.dll",
        "UnhandledExceptionFilter"
    ));
    let (mut mem, mut cpu, mut runtime) = startup();
    let target = 0x1234_5678_1234_5678;
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetUnhandledExceptionFilter",
            &[target]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetUnhandledExceptionFilter",
            &[0]
        ),
        target
    );
    let handle = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "AddVectoredExceptionHandler",
        &[1 << 32, target],
    );
    assert_eq!(handle, 0x7fff8000);
    // UINT first flag truncates, but HANDLE removal keeps all 64 bits.
    assert_eq!(
        runtime.trace.back().unwrap().to_string(),
        format!("KERNEL32.dll!AddVectoredExceptionHandler args=[0, {target:x}] -> 0x{handle:x}")
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "RemoveVectoredExceptionHandler",
            &[handle | (1 << 32)]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "RemoveVectoredExceptionHandler",
            &[handle]
        ),
        1
    );
}

#[test]
fn exception_contract_preserves_return_frame_preflight_and_monotonic_handler_limits() {
    let (mut mem, mut cpu, mut runtime) = startup();
    cpu.regs[4] = 0x123000;
    cpu.regs[1] = 0;
    cpu.regs[2] = BASE;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch(
            "kernel32.dll",
            "AddVectoredExceptionHandler",
            &mut cpu,
            &mut mem
        )
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    startup_call(&mut mem, &mut cpu, &mut runtime, "SetLastError", &[77]);
    for n in 0..256u64 {
        let handle = startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "AddVectoredExceptionHandler",
            &[0, BASE],
        );
        assert_eq!(handle, 0x7fff8000 + n);
        assert_eq!(
            startup_call(
                &mut mem,
                &mut cpu,
                &mut runtime,
                "RemoveVectoredExceptionHandler",
                &[handle]
            ),
            1
        );
    }
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        77
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "AddVectoredExceptionHandler",
            &[0, BASE]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        8
    );
    // Null target is checked before cumulative exhaustion.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "AddVectoredExceptionHandler",
            &[0, 0]
        ),
        0
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetLastError", &[]),
        87
    );
}
