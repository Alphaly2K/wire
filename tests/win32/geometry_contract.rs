//! Independent rectangle arithmetic, aliasing and PE32 frame-order baseline.
use super::*;

fn rect(v: &Vm, p: u32) -> [i32; 4] {
    std::array::from_fn(|i| v.mem.read_u32(p + i as u32 * 4).unwrap() as i32)
}

#[test]
fn geometry_contract_pe32_keeps_signed_points_overlap_and_empty_rectangles() {
    let mut v = Vm::new();
    let a = DATA;
    let b = DATA + 64;
    let out = DATA + 128;
    assert_eq!(
        v.call(
            "user32.dll",
            "SetRect",
            &[a, (-5i32) as u32, (-7i32) as u32, 10, 20]
        ),
        1
    );
    assert_eq!(
        v.call(
            "user32.dll",
            "PtInRect",
            &[a, (-5i32) as u32, (-7i32) as u32]
        ),
        1
    );
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 256 + 16);
    assert_eq!(v.call("user32.dll", "PtInRect", &[a, 10, 20]), 0);
    assert_eq!(v.call("user32.dll", "CopyRect", &[a + 4, a]), 1);
    assert_eq!(rect(&v, a + 4), [-5, -7, 10, 20]);
    v.call("user32.dll", "SetRect", &[a, 1, 2, 3, 4]);
    assert_eq!(
        v.call("user32.dll", "InflateRect", &[a, (-2i32) as u32, 3]),
        1
    );
    assert_eq!(rect(&v, a), [3, -1, 1, 7]);
    assert_eq!(v.call("user32.dll", "IsRectEmpty", &[a]), 1);
    v.call("user32.dll", "SetRect", &[a, (-5i32) as u32, 0, 5, 10]);
    v.call("user32.dll", "SetRect", &[b, 5, 0, 10, 10]);
    assert_eq!(v.call("user32.dll", "IntersectRect", &[out, a, b]), 0);
    assert_eq!(rect(&v, out), [0; 4]);
    assert_eq!(v.call("user32.dll", "UnionRect", &[out, out, b]), 1);
    assert_eq!(v.call("user32.dll", "EqualRect", &[out, b]), 1);
    assert_eq!(v.call("user32.dll", "SetRectEmpty", &[b]), 1);
    assert_eq!(v.call("user32.dll", "UnionRect", &[out, b, b]), 0);
    assert_eq!(rect(&v, out), [0; 4]);
    v.call(
        "user32.dll",
        "SetRect",
        &[
            a,
            i32::MAX as u32,
            i32::MIN as u32,
            i32::MAX as u32,
            i32::MIN as u32,
        ],
    );
    assert_eq!(v.call("user32.dll", "OffsetRect", &[a, 1, 1]), 1);
    assert_eq!(
        rect(&v, a),
        [i32::MIN, i32::MIN + 1, i32::MIN, i32::MIN + 1]
    );
}

#[test]
fn geometry_contract_pe32_keeps_adjust_error_and_handler_then_return_order() {
    let mut v = Vm::new();
    for name in [
        "AdjustWindowRect",
        "AdjustWindowRectEx",
        "AdjustWindowRectExForDpi",
    ] {
        v.call("user32.dll", "SetRect", &[DATA, 0, 0, 200, 100]);
        v.k("SetLastError", &[77]);
        let args = [DATA, 0xc40000, 0, 0, 96];
        let n = if name == "AdjustWindowRect" {
            3
        } else if name.ends_with("ForDpi") {
            5
        } else {
            4
        };
        assert_eq!(v.call("user32.dll", name, &args[..n]), 1);
        assert_eq!(rect(&v, DATA), [-4, -27, 204, 104]);
        assert_eq!(v.state.last_error, 77);
    }
    assert_eq!(
        v.call("user32.dll", "AdjustWindowRectExForDpi", &[1, 0, 0, 0, 144]),
        0
    );
    assert_eq!(v.state.last_error, 120);
    v.call("user32.dll", "SetRect", &[DATA, 0x11223344, 2, 3, 4]);
    let slot = STACK_BASE + STACK_SIZE - 256;
    assert_eq!(v.call("user32.dll", "CopyRect", &[slot, DATA]), 1);
    assert_eq!(v.cpu.eip, 0x11223344);
    assert_eq!(v.cpu.esp(), slot + 12);
}

#[test]
fn geometry_contract_pe32_copy_fault_preserves_accessible_output_prefix() {
    let mut v = Vm::new();
    v.call("user32.dll", "SetRect", &[DATA, 1, 2, 3, 4]);
    let output = DATA + 0xfff8;
    v.mem.write_bytes(output, &[0xa5; 8]).unwrap();
    assert!(v
        .try_call("user32.dll", "CopyRect", &[output, DATA])
        .is_err());
    let mut bytes = [0; 8];
    v.mem.read_bytes(output, &mut bytes).unwrap();
    assert_eq!(bytes, [0xa5; 8]);
}
