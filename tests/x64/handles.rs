//! Win64 handle ABI and return-frame baselines independent of the catalog.
use super::*;

#[test]
fn handle_contract_baseline_keeps_win64_widths_output_alias_and_provider_scope() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let high = 0xface0000_00000000;
    assert_eq!(
        startup_dll_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "kernelbase.dll",
            "GetStdHandle",
            &[high | 0xfffffff5]
        ),
        0x101
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "DuplicateHandle",
            &[
                u64::MAX,
                0x101,
                u64::MAX,
                STACK + 0x100,
                high,
                high,
                high | 2
            ]
        ),
        1
    );
    let alias = mem.read_int(STACK + 0x100, 8).unwrap();
    assert_eq!(alias, 0x7fff3000);
    // BOOL and access/options are DWORD, while handle and output are pointers.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[alias, STACK + 0x1f88]
        ),
        1
    );
    assert_eq!(mem.read_int(STACK + 0x1f88, 4).unwrap(), 0);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetHandleInformation",
            &[alias, high | 1, high | 1]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[alias, STACK + 0x100]
        ),
        1
    );
    assert_eq!(mem.read_int(STACK + 0x100, 4).unwrap(), 1);
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "SetStdHandle",
            &[high | 0xfffffff5, high | 0x1234]
        ),
        1
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetStdHandle",
            &[0xfffffff5]
        ),
        high | 0x1234
    );
    // High handle bits are not discarded during resource lookup.
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "CloseHandle",
            &[high | alias]
        ),
        0
    );
    assert_eq!(
        startup_call(
            &mut mem,
            &mut cpu,
            &mut runtime,
            "GetHandleInformation",
            &[alias, STACK + 0x100]
        ),
        1
    );
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "CloseHandle", &[alias]),
        1
    );
    for name in [
        "GetStdHandle",
        "SetStdHandle",
        "CloseHandle",
        "DuplicateHandle",
        "GetHandleInformation",
        "SetHandleInformation",
    ] {
        assert!(!wire::x64::runtime::import_supported("user32.dll", name));
    }
}

#[test]
fn handle_contract_baseline_keeps_win64_close_after_return_frame_preflight() {
    let (mut mem, mut cpu, mut runtime) = startup();
    let endpoint = startup_call(
        &mut mem,
        &mut cpu,
        &mut runtime,
        "GetStdHandle",
        &[0xfffffff5],
    );
    cpu.regs[4] = STACK + 8192;
    cpu.regs[1] = endpoint;
    let before = cpu.clone();
    let calls = runtime.api_calls;
    assert!(runtime
        .dispatch("kernel32.dll", "CloseHandle", &mut cpu, &mut mem)
        .is_err());
    assert_eq!(cpu, before);
    assert_eq!(runtime.api_calls, calls);
    assert_eq!(
        startup_call(&mut mem, &mut cpu, &mut runtime, "GetFileType", &[endpoint]),
        3
    );
}
