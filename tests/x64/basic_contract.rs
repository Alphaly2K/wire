//! Preserve the narrow Win64 processor-count subset and low-WORD carrier.
use super::*;

#[test]
fn basic_contract_processor_counts_consume_low_word_and_keep_saved_frame() {
    let (mut mem, mut cpu, mut rt) = startup();
    for name in [
        "GetActiveProcessorGroupCount",
        "GetMaximumProcessorGroupCount",
    ] {
        startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
        assert_eq!(startup_call(&mut mem, &mut cpu, &mut rt, name, &[]), 1);
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            123
        );
    }
    for name in ["GetActiveProcessorCount", "GetMaximumProcessorCount"] {
        for value in [0, 0xffff, 0x12340000, 0xabcdef12_3456ffff] {
            startup_call(&mut mem, &mut cpu, &mut rt, "SetLastError", &[123]);
            assert_eq!(startup_call(&mut mem, &mut cpu, &mut rt, name, &[value]), 1);
            assert!(rt
                .trace
                .back()
                .unwrap()
                .contains(&format!("args=[{value:x}]")));
            assert_eq!(
                startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
                123
            );
        }
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, name, &[0x12340001]),
            0
        );
        assert_eq!(
            startup_call(&mut mem, &mut cpu, &mut rt, "GetLastError", &[]),
            87
        );
    }
}

#[test]
fn basic_contract_win64_preserves_absent_helpers_and_processor_providers() {
    for name in [
        "GetActiveProcessorCount",
        "GetMaximumProcessorCount",
        "GetActiveProcessorGroupCount",
        "GetMaximumProcessorGroupCount",
    ] {
        assert!(wire::x64::runtime::import_supported("kernel32.dll", name));
        assert!(wire::x64::runtime::import_supported("kernelbase.dll", name));
        assert!(!wire::x64::runtime::import_supported("ole32.dll", name));
    }
    for name in [
        "CompareStringOrdinal",
        "lstrcmpW",
        "lstrcmpA",
        "lstrcmpiW",
        "lstrcmpiA",
        "lstrlenA",
        "lstrlenW",
        "lstrlen",
        "lstrcpyA",
        "lstrcpyW",
        "lstrcatA",
        "lstrcatW",
        "lstrcpynA",
        "lstrcpynW",
        "IsBadReadPtr",
        "IsBadWritePtr",
        "IsBadCodePtr",
        "FlushInstructionCache",
        "MulDiv",
        "OpenProcess",
        "GetProcessAffinityMask",
        "SetProcessAffinityMask",
        "SetThreadIdealProcessor",
        "GetProcessId",
        "GetExitCodeProcess",
    ] {
        assert!(!wire::x64::runtime::import_supported("kernel32.dll", name));
        assert!(!wire::x64::runtime::import_supported(
            "kernelbase.dll",
            name
        ));
    }
}
