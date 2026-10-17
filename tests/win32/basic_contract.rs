//! Legacy common-helper behavior fixed before separating its responsibilities.
use super::*;

#[test]
fn basic_contract_strings_keep_overlap_capacity_and_failure_outputs() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, b"abc\0").unwrap();
    assert_eq!(v.k("lstrcpyA", &[DATA + 1, DATA]), DATA + 1);
    let mut bytes = [0; 5];
    v.mem.read_bytes(DATA, &mut bytes).unwrap();
    assert_eq!(&bytes, b"aabc\0");
    v.wide(DATA, "ab");
    assert_eq!(v.k("lstrcatW", &[DATA, DATA]), DATA);
    assert_eq!(v.mem.read_wstr(DATA, 10).unwrap(), "abab");
    for capacity in [0, u32::MAX, 0x80000000] {
        v.mem.write_u32(DATA + 0x100, 0x55555555).unwrap();
        assert_eq!(v.k("lstrcpynW", &[DATA + 0x100, 0, capacity]), DATA + 0x100);
        assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 0x55555555);
    }
    let edge = DATA + 0xffff;
    v.mem.write_u8(edge, b'x').unwrap();
    assert_eq!(v.k("lstrcpynA", &[DATA, edge, 2]), DATA);
    assert_eq!(v.mem.read_u16(DATA).unwrap(), b'x' as u16);
    v.mem.write_u32(DATA, 0x55555555).unwrap();
    assert_eq!(v.k("lstrcpyA", &[DATA, edge]), 0);
    assert_eq!(v.state.last_error, 87);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0x55555555);
    v.wide(DATA + 0x100, "z");
    let slot = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.k("lstrcpyW", &[slot, DATA + 0x100]), slot);
    assert_eq!(v.cpu.eip, b'z' as u32);
    assert_eq!(v.cpu.esp(), slot + 12);
}

#[test]
fn basic_contract_comparison_keeps_null_counts_and_single_unit_case_mapping() {
    let mut v = Vm::new();
    v.k("SetLastError", &[123]);
    assert_eq!(v.k("lstrlen", &[0]), 0);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 248);
    v.mem.write_bytes(DATA, b"abc\0").unwrap();
    assert_eq!(v.k("lstrlen", &[DATA]), 3);
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 248);
    assert_eq!(v.k("lstrcmpA", &[0, 0]), 0);
    assert_eq!(v.state.last_error, 123);
    v.wide(DATA, "ß");
    v.wide(DATA + 0x100, "SS");
    assert_eq!(v.k("lstrcmpiW", &[DATA, DATA + 0x100]), 1);
    v.mem.write_u16(DATA, b'a' as u16).unwrap();
    v.mem.write_u16(DATA + 2, 0xd800).unwrap();
    v.mem.write_u16(DATA + 0x100, b'A' as u16).unwrap();
    v.mem.write_u16(DATA + 0x102, 0xd800).unwrap();
    assert_eq!(
        v.k("CompareStringOrdinal", &[DATA, 2, DATA + 0x100, 2, 1]),
        2
    );
    assert_eq!(v.k("CompareStringOrdinal", &[0, 0, DATA, 0, 0]), 0);
    assert_eq!(v.state.last_error, 87);
}

#[test]
fn basic_contract_process_access_error_order_and_aliases_are_preserved() {
    let mut v = Vm::new();
    for (pid, error) in [(0, 87), (4243, 5)] {
        assert_eq!(v.k("OpenProcess", &[0x1600, u32::MAX, pid]), 0);
        assert_eq!(v.state.last_error, error);
    }
    let handle = v.k("OpenProcess", &[0x200, u32::MAX, 4242]);
    assert_ne!(handle, 0);
    assert_eq!(v.k("GetExitCodeProcess", &[handle, 0]), 0);
    assert_eq!(v.state.last_error, 5); // Access is checked before the output pointer.
    assert_eq!(v.k("SetProcessAffinityMask", &[handle, 1]), 1);
    assert_eq!(v.k("GetExitCodeProcess", &[0xdeadbeef, 0]), 0);
    assert_eq!(v.state.last_error, 6);
    let slot = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.k("GetProcessAffinityMask", &[u32::MAX, slot, slot]), 1);
    assert_eq!(v.cpu.eip, 1);
    assert_eq!(v.cpu.esp(), slot + 16);
    assert_eq!(v.k("SetThreadIdealProcessor", &[0xdeadbeef, 1]), u32::MAX);
    assert_eq!(v.state.last_error, 6);
    for name in ["GetActiveProcessorCount", "GetMaximumProcessorCount"] {
        assert_eq!(v.k(name, &[0x12340000]), 0);
        assert_eq!(v.state.last_error, 87);
        assert_eq!(v.k(name, &[0xffff]), 1);
    }
    assert_eq!(v.k("CloseHandle", &[handle]), 1);
    for name in [
        "GetActiveProcessorGroupCount",
        "GetMaximumProcessorGroupCount",
    ] {
        assert_eq!(v.k(name, &[]), 0);
        assert_eq!(v.state.last_error, 120);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252);
    }
}

#[test]
fn basic_contract_pointer_checks_and_muldiv_preserve_success_last_error() {
    let mut v = Vm::new();
    v.k("SetLastError", &[123]);
    for name in ["IsBadReadPtr", "IsBadWritePtr"] {
        assert_eq!(v.k(name, &[u32::MAX, 0]), 0);
        assert_eq!(v.k(name, &[DATA + 0xfffe, 4]), 1);
        assert_eq!(v.state.last_error, 123);
    }
    assert_eq!(v.k("IsBadCodePtr", &[0]), 1);
    assert_eq!(
        v.k("FlushInstructionCache", &[u32::MAX, 0xdeadbeef, u32::MAX]),
        1
    );
    for args in [[1, 1, 0], [i32::MAX as u32, 2, 1], [(-1i32) as u32, 1, 2]] {
        assert_eq!(v.k("MulDiv", &args), u32::MAX);
        assert_eq!(v.state.last_error, 123);
    }
    assert_eq!(v.k("FlushInstructionCache", &[0, 0, 0]), 0);
    assert_eq!(v.state.last_error, 6);
}
