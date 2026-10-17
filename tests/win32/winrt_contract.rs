//! WinRT is still unknown ABI in the PE32 service set.
use super::*;

#[test]
fn winrt_contract_keeps_pe32_unknown_abi_and_does_not_invent_cleanup() {
    let mut v = Vm::new();
    for dll in [
        "combase.dll",
        "api-ms-win-core-winrt-string-l1-1-0.dll",
        "api-ms-win-core-winrt-l1-1-0.dll",
        "ole32.dll",
        "kernel32.dll",
    ] {
        for name in [
            "WindowsCreateString",
            "WindowsCreateStringReference",
            "WindowsDeleteString",
            "WindowsDuplicateString",
            "WindowsGetStringLen",
            "WindowsIsStringEmpty",
            "WindowsGetStringRawBuffer",
            "WindowsStringHasEmbeddedNull",
            "WindowsCompareStringOrdinal",
            "RoGetActivationFactory",
            "RoActivateInstance",
        ] {
            assert_eq!(
                v.call(dll, name, &[DATA, 1, DATA + 0x100, DATA + 0x200]),
                if dll == "ole32.dll" { 0x80004001 } else { 0 },
                "{dll}!{name}"
            );
            assert_eq!(v.state.last_error, 120);
            assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252, "{dll}!{name}");
        }
    }
}
