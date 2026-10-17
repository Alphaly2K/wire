//! Independent PE32 BSTR heap sharing, HRESULT and ABI-only baseline.
use super::*;

#[test]
fn bstr_contract_keeps_pe32_heap_membership_limits_and_foreign_prefixes() {
    let mut v = Vm::new();
    v.mem.write_u32(DATA, 5).unwrap();
    assert_eq!(v.call("oleaut32.dll", "SysStringLen", &[DATA + 4]), 2);
    assert_eq!(v.call("oleaut32.dll", "SysStringByteLen", &[DATA + 4]), 5);
    assert_eq!(v.call("oleaut32.dll", "SysFreeString", &[DATA + 4]), 0);
    assert_eq!(v.state.last_error, 87);
    v.wide(DATA, "abc");
    let pointer = v.call("oleaut32.dll", "SysAllocStringLen", &[DATA, 3]);
    assert_ne!(pointer, 0);
    assert_eq!(v.k("HeapSize", &[v.state.process_heap, 0, pointer - 4]), 12);
    assert_eq!(v.k("HeapFree", &[v.state.process_heap, 0, pointer - 4]), 1);
    assert_eq!(v.call("oleaut32.dll", "SysFreeString", &[pointer]), 0);
    assert_eq!(v.state.last_error, 87);
    assert_eq!(
        v.call("oleaut32.dll", "SysAllocStringLen", &[0xdead0000, 0x100001]),
        0
    );
    assert_eq!(v.state.last_error, 8);
    let pointer = v.call("oleaut32.dll", "SysAllocStringLen", &[DATA, 3]);
    v.mem.write_u32(DATA + 0x100, pointer).unwrap();
    assert_eq!(
        v.call(
            "oleaut32.dll",
            "SysReAllocStringLen",
            &[DATA + 0x100, 0, 0x100001]
        ),
        0
    );
    assert_eq!(v.state.last_error, 8);
    assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), pointer);
    assert_eq!(
        v.call("oleaut32.dll", "SysReAllocString", &[DATA + 0x100, 0]),
        1
    );
    assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 0);
}

#[test]
fn bstr_contract_keeps_pe32_byte_allocator_abi_only_and_wrong_provider_cleanup() {
    let mut v = Vm::new();
    assert_eq!(
        v.call("oleaut32.dll", "SysAllocStringByteLen", &[DATA, 3]),
        0
    );
    assert_eq!(v.state.last_error, 120);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252 + 8);
    for dll in ["kernel32.dll", "combase.dll", "user32.dll"] {
        for name in [
            "SysAllocString",
            "SysAllocStringLen",
            "SysAllocStringByteLen",
            "SysReAllocString",
            "SysReAllocStringLen",
            "SysFreeString",
            "SysStringLen",
            "SysStringByteLen",
        ] {
            assert_eq!(v.call(dll, name, &[DATA, 3, 0]), 0, "{dll}!{name}");
            assert_eq!(v.state.last_error, 120);
            assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252, "{dll}!{name}");
        }
    }
}
