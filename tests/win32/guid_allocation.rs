//! Independent allocation/fault baseline for the remaining GUID helpers.
use super::*;

const GUID: [u8; 16] = [
    0x33, 0x22, 0x11, 0, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
];
const TEXT: &str = "{00112233-4455-6677-8899-AABBCCDDEEFF}";

#[test]
fn guid_allocation_keeps_bounded_heap_output_zeroing_and_error_precedence() {
    for name in ["StringFromCLSID", "StringFromIID"] {
        let mut v = Vm::new();
        v.state.process_heap = v.state.heap.create(80);
        v.mem.write_bytes(DATA, &GUID).unwrap();
        let output = DATA + 0x100;
        v.mem.write_u32(output, 0xdeadbeef).unwrap();
        assert_eq!(v.call("ole32.dll", name, &[0, output]), 0x80004003);
        assert_eq!(v.mem.read_u32(output).unwrap(), 0xdeadbeef);
        v.k("SetLastError", &[123]);
        assert_eq!(v.call("ole32.dll", name, &[DATA, output]), 0);
        let first = v.mem.read_u32(output).unwrap();
        assert_eq!(v.mem.read_wstr(first, 39).unwrap(), TEXT);
        assert_eq!(v.state.heap.size(v.state.process_heap, first), Some(78));
        assert_eq!(v.state.last_error, 123);
        assert_eq!(v.call("ole32.dll", name, &[DATA, output]), 0x8007000e);
        assert_eq!(v.mem.read_u32(output).unwrap(), 0);
        assert_eq!(v.state.last_error, 0x8007000e);
        assert_eq!(v.mem.read_wstr(first, 39).unwrap(), TEXT);
        v.call("ole32.dll", "CoTaskMemFree", &[first]);
        assert_eq!(v.call("ole32.dll", name, &[DATA, output]), 0);
        let next = v.mem.read_u32(output).unwrap();
        assert_eq!(v.mem.read_wstr(next, 39).unwrap(), TEXT);
        v.call("ole32.dll", "CoTaskMemFree", &[next]);
    }
}

#[test]
fn guid_allocation_keeps_input_and_return_slot_aliases_and_entropy_faults() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, &GUID).unwrap();
    assert_eq!(v.call("ole32.dll", "StringFromIID", &[DATA, DATA]), 0);
    let first = v.mem.read_u32(DATA).unwrap();
    assert_eq!(v.mem.read_wstr(first, 39).unwrap(), TEXT);
    v.call("ole32.dll", "CoTaskMemFree", &[first]);
    v.mem.write_bytes(DATA, &GUID).unwrap();
    let slot = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.call("ole32.dll", "StringFromCLSID", &[DATA, slot]), 0);
    let pointer = v.mem.read_u32(slot).unwrap();
    assert_eq!(v.cpu.eip, pointer);
    assert_eq!(v.cpu.esp(), slot + 12);
    assert_eq!(v.mem.read_wstr(pointer, 39).unwrap(), TEXT);
    v.call("ole32.dll", "CoTaskMemFree", &[pointer]);
    let edge = DATA + 0xfff8;
    v.mem.write_bytes(edge, &[0x55; 8]).unwrap();
    assert_eq!(v.call("ole32.dll", "CoCreateGuid", &[edge]), 0x80004003);
    let mut bytes = [0; 8];
    v.mem.read_bytes(edge, &mut bytes).unwrap();
    assert_eq!(bytes, [0x55; 8]);
    assert_eq!(v.state.last_error, 0x80004003);
}
