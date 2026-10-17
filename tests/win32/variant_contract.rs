//! Preserve scalar bytes, deep-copy rollback, BYREF and HRESULT ordering.
use super::*;

#[test]
fn variant_contract_keeps_scalar_bytes_type_widths_and_last_error() {
    let mut v = Vm::new();
    let source = DATA;
    let output = DATA + 0x100;
    let mut bytes = [0x55; 16];
    bytes[..2].copy_from_slice(&3u16.to_le_bytes());
    bytes[8..12].copy_from_slice(&42u32.to_le_bytes());
    v.mem.write_bytes(source, &bytes).unwrap();
    v.call("oleaut32.dll", "VariantInit", &[output]);
    v.k("SetLastError", &[123]);
    for (name, args, cleanup) in [
        ("VariantCopy", vec![output, source], 8),
        (
            "VariantChangeType",
            vec![output, source, u32::MAX, 0x12340003],
            16,
        ),
    ] {
        assert_eq!(v.call("oleaut32.dll", name, &args), 0);
        assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252 + cleanup);
        let mut actual = [0; 16];
        v.mem.read_bytes(output, &mut actual).unwrap();
        assert_eq!(actual, bytes);
        assert_eq!(v.state.last_error, 123);
    }
    assert_eq!(v.call("oleaut32.dll", "VariantClear", &[output]), 0);
    let mut actual = [0; 16];
    v.mem.read_bytes(output, &mut actual).unwrap();
    bytes[..2].fill(0);
    assert_eq!(actual, bytes); // Clear changes the tag, not the inactive payload.
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 252 + 4);
}

#[test]
fn variant_contract_keeps_deep_copy_rollback_in_a_bounded_heap_and_self_alias() {
    let mut v = Vm::new();
    v.state.process_heap = v.state.heap.create(64);
    let source = DATA + 0x100;
    let output = DATA + 0x200;
    let pointer = v.call("oleaut32.dll", "SysAllocStringLen", &[0, 8]);
    assert_ne!(pointer, 0);
    v.mem.write_u16(source, 8).unwrap();
    v.mem.write_u32(source + 8, pointer).unwrap();
    v.mem.write_bytes(output, &[0x55; 16]).unwrap();
    v.mem.write_u16(output, 9).unwrap();
    for _ in 0..8 {
        assert_eq!(
            v.call("oleaut32.dll", "VariantCopy", &[output, source]),
            0x80020008
        );
        assert_eq!(v.state.last_error, 0x80020008);
        assert_eq!(v.mem.read_u16(output).unwrap(), 9);
        assert_eq!(v.mem.read_u32(output + 8).unwrap(), 0x55555555);
        assert_eq!(v.call("oleaut32.dll", "SysStringLen", &[pointer]), 8);
    }
    assert_eq!(v.call("oleaut32.dll", "VariantCopy", &[source, source]), 0);
    let next = v.mem.read_u32(source + 8).unwrap();
    assert_ne!(next, pointer);
    assert!(!v.state.heap.valid(v.state.process_heap, pointer - 4));
    assert!(v.state.heap.valid(v.state.process_heap, next - 4));
    assert_eq!(v.call("oleaut32.dll", "VariantClear", &[source]), 0);
    assert!(!v.state.heap.valid(v.state.process_heap, next - 4));
    assert_ne!(v.k("HeapAlloc", &[v.state.process_heap, 0, 64]), 0);
}

#[test]
fn variant_contract_keeps_byref_lifetime_and_error_precedence() {
    let mut v = Vm::new();
    let pointer = v.call("oleaut32.dll", "SysAllocStringLen", &[0, 3]);
    v.mem.write_bytes(DATA, &[0x55; 16]).unwrap();
    v.mem.write_u16(DATA, 0x4008).unwrap();
    v.mem.write_u32(DATA + 8, pointer).unwrap();
    assert_eq!(v.call("oleaut32.dll", "VariantClear", &[DATA]), 0);
    assert_eq!(v.mem.read_u16(DATA).unwrap(), 0);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), pointer);
    assert!(v.state.heap.valid(v.state.process_heap, pointer - 4));
    for (name, args, expected) in [
        ("VariantCopy", vec![0, 0xdead0000], 0x80070057),
        ("VariantCopy", vec![DATA, 0xdead0000], 0x80070057),
        ("VariantInit", vec![DATA + 0xffff], 0x80070057),
        ("VariantChangeType", vec![DATA, DATA, 0, 9], 0x80020005),
    ] {
        assert_eq!(v.call("oleaut32.dll", name, &args), expected);
        assert_eq!(v.state.last_error, expected);
    }
    assert_eq!(v.call("oleaut32.dll", "SysFreeString", &[pointer]), 0);
}
