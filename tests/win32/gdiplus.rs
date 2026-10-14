//! Existing behavioral tests grouped by gdiplus; expectations remain independent of API catalogs.

use super::*;

fn gdip(v: &mut Vm, name: &str, args: &[u32]) -> u32 {
    v.call("gdiplus.dll", name, args)
}

fn matrix_values(v: &mut Vm, handle: u32) -> [f32; 6] {
    assert_eq!(gdip(v, "GdipGetMatrixElements", &[handle, DATA + 0x400]), 0);
    std::array::from_fn(|i| f32::from_bits(v.mem.read_u32(DATA + 0x400 + i as u32 * 4).unwrap()))
}

#[test]
fn gdiplus_exports_startup_allocation_and_unported_status_preserve_x86_abi() {
    let mut v = Vm::new();
    v.mem.write_bytes(DATA, b"gdiplus.dll\0").unwrap();
    let module = v.k("LoadLibraryA", &[DATA]);
    assert_ne!(module, 0);
    assert_ne!(dll_export(&mut v, module, "GdipCreateMatrix2"), 0);
    assert_eq!(dll_export(&mut v, module, "GdipMadeUpExport"), 0);
    assert_eq!(wire::gdiplus::stack_bytes("GdipCreateEffect"), Some(20)); // GUID by value
    assert_eq!(
        wire::gdiplus::stack_bytes("GdipDrawImagePointsRect"),
        Some(48)
    );
    v.mem.write_bytes(DATA + 0x100, &[0; 16]).unwrap();
    v.mem.write_u32(DATA + 0x100, 1).unwrap();
    v.mem.write_u32(DATA + 0x200, 0x1234).unwrap();
    assert_eq!(
        gdip(&mut v, "GdiplusStartup", &[DATA + 0x200, DATA + 0x100, 0]),
        0
    );
    let first = v.mem.read_u32(DATA + 0x200).unwrap();
    assert_ne!(first, 0);
    assert_eq!(
        gdip(&mut v, "GdiplusStartup", &[DATA + 0x204, DATA + 0x100, 0]),
        0
    );
    assert_ne!(first, v.mem.read_u32(DATA + 0x204).unwrap());
    v.mem.write_u32(DATA + 0x100, 3).unwrap();
    assert_eq!(
        gdip(&mut v, "GdiplusStartup", &[DATA + 0x200, DATA + 0x100, 0]),
        17
    );
    assert_eq!(v.mem.read_u32(DATA + 0x200).unwrap(), first);
    v.mem.write_u32(DATA + 0x100, 1).unwrap();
    v.mem.write_u32(DATA + 0x108, 1).unwrap();
    assert_eq!(
        gdip(&mut v, "GdiplusStartup", &[DATA + 0x200, DATA + 0x100, 0]),
        2
    );
    assert_eq!(
        gdip(
            &mut v,
            "GdiplusStartup",
            &[DATA + 0x200, DATA + 0x100, DATA + 0x300]
        ),
        6
    );
    let p = gdip(&mut v, "GdipAlloc", &[31]);
    assert_ne!(p, 0);
    let mut zero = [1; 31];
    v.mem.read_bytes(p, &mut zero).unwrap();
    assert_eq!(zero, [0; 31]);
    assert_eq!(gdip(&mut v, "GdipFree", &[p]), 0);
    assert_eq!(v.k("HeapSize", &[v.state.process_heap, 0, p]), u32::MAX);
    v.mem.write_u32(DATA + 0x300, 0x76543210).unwrap();
    assert_eq!(
        gdip(&mut v, "GdipCreateSolidFill", &[0xff00aa00, DATA + 0x300]),
        6
    );
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), 0x76543210);
    assert!(v.log.recent.back().unwrap().contains("UNIMPLEMENTED"));
    assert_eq!(
        gdip(
            &mut v,
            "GdipCreateMatrix2",
            &[1f32.to_bits(), 0, 0, 1f32.to_bits(), 0, 0, DATA + 0x300]
        ),
        0
    );
    assert_eq!(v.cpu.esp(), STACK_BASE + STACK_SIZE - 224); // return + seven DWORDs
}

#[test]
fn gdiplus_matrix_order_clone_inverse_and_object_lifetime_match_affine_geometry() {
    let mut v = Vm::new();
    assert_eq!(gdip(&mut v, "GdipCreateMatrix", &[DATA]), 0);
    let m = v.mem.read_u32(DATA).unwrap();
    assert_eq!(matrix_values(&mut v, m), [1., 0., 0., 1., 0., 0.]);
    assert_eq!(
        gdip(
            &mut v,
            "GdipTranslateMatrix",
            &[m, 3f32.to_bits(), 5f32.to_bits(), 1]
        ),
        0
    );
    assert_eq!(gdip(&mut v, "GdipCloneMatrix", &[m, DATA + 4]), 0);
    let clone = v.mem.read_u32(DATA + 4).unwrap();
    assert_eq!(
        gdip(
            &mut v,
            "GdipScaleMatrix",
            &[m, 2f32.to_bits(), 4f32.to_bits(), 1]
        ),
        0
    );
    assert_eq!(matrix_values(&mut v, m), [2., 0., 0., 4., 6., 20.]);
    assert_eq!(
        gdip(
            &mut v,
            "GdipScaleMatrix",
            &[clone, 2f32.to_bits(), 4f32.to_bits(), 0]
        ),
        0
    );
    assert_eq!(matrix_values(&mut v, clone), [2., 0., 0., 4., 3., 5.]);
    assert_eq!(gdip(&mut v, "GdipInvertMatrix", &[m]), 0);
    assert_eq!(matrix_values(&mut v, m), [0.5, 0., 0., 0.25, -3., -5.]);
    assert_eq!(gdip(&mut v, "GdipMultiplyMatrix", &[m, clone, 1]), 0);
    assert_eq!(matrix_values(&mut v, m), [1., 0., 0., 1., -3., -15.]);
    // A prepend quarter turn rotates the basis but leaves its translation.
    assert_eq!(
        gdip(&mut v, "GdipRotateMatrix", &[m, 90f32.to_bits(), 0]),
        0
    );
    let rotated = matrix_values(&mut v, m);
    for (got, expected) in rotated.into_iter().zip([0., 1., -1., 0., -3., -15.]) {
        assert!((got - expected).abs() < 1e-6);
    }
    assert_eq!(
        gdip(
            &mut v,
            "GdipSetMatrixElements",
            &[m, 1f32.to_bits(), 0, 0, 1f32.to_bits(), 0, 0]
        ),
        0
    );
    assert_eq!(
        gdip(
            &mut v,
            "GdipShearMatrix",
            &[m, 2f32.to_bits(), 3f32.to_bits(), 1]
        ),
        0
    );
    assert_eq!(matrix_values(&mut v, m), [1., 3., 2., 1., 0., 0.]);
    let old = matrix_values(&mut v, m);
    assert_eq!(
        gdip(
            &mut v,
            "GdipTranslateMatrix",
            &[m, 1f32.to_bits(), 2f32.to_bits(), 2]
        ),
        2
    );
    assert_eq!(matrix_values(&mut v, m), old);
    assert_eq!(gdip(&mut v, "GdipDeleteMatrix", &[m]), 0);
    assert_eq!(gdip(&mut v, "GdipGetMatrixElements", &[m, DATA + 0x400]), 2);
    assert_eq!(gdip(&mut v, "GdipDeleteMatrix", &[m]), 2);
    assert_eq!(
        gdip(&mut v, "GdipGetMatrixElements", &[0xfffffff0, DATA + 0x400]),
        2
    );
}

#[test]
fn gdiplus_matrix_singular_threshold_bitwise_identity_and_checked_points() {
    let mut v = Vm::new();
    assert_eq!(
        gdip(
            &mut v,
            "GdipCreateMatrix2",
            &[
                1f32.to_bits(),
                1f32.to_bits(),
                1f32.to_bits(),
                1f32.to_bits(),
                0,
                0,
                DATA
            ]
        ),
        0
    );
    let m = v.mem.read_u32(DATA).unwrap();
    assert_eq!(gdip(&mut v, "GdipIsMatrixInvertible", &[m, DATA + 4]), 0);
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 0);
    let old = matrix_values(&mut v, m);
    assert_eq!(gdip(&mut v, "GdipInvertMatrix", &[m]), 2);
    assert_eq!(matrix_values(&mut v, m), old);
    // 1e-5f32 is just below Wine's double threshold; the next F32 is above it.
    // The off-diagonal 1 forces the general determinant path.
    for (det_bits, expected) in [(1e-5f32.to_bits(), 0), (1e-5f32.to_bits() + 1, 1)] {
        assert_eq!(
            gdip(
                &mut v,
                "GdipSetMatrixElements",
                &[m, det_bits, 1f32.to_bits(), 0, 1f32.to_bits(), 0, 0]
            ),
            0
        );
        assert_eq!(gdip(&mut v, "GdipIsMatrixInvertible", &[m, DATA + 4]), 0);
        assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), expected);
        assert_eq!(
            gdip(&mut v, "GdipInvertMatrix", &[m]),
            if expected == 0 { 2 } else { 0 }
        );
    }
    assert_eq!(
        gdip(
            &mut v,
            "GdipSetMatrixElements",
            &[
                m,
                1f32.to_bits(),
                (-0f32).to_bits(),
                0,
                1f32.to_bits(),
                0,
                0
            ]
        ),
        0
    );
    assert_eq!(gdip(&mut v, "GdipIsMatrixIdentity", &[m, DATA + 4]), 0);
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 0); // Wine compares the bytes, including -0
    assert_eq!(
        gdip(
            &mut v,
            "GdipSetMatrixElements",
            &[
                m,
                2f32.to_bits(),
                0,
                0,
                3f32.to_bits(),
                10f32.to_bits(),
                (-4f32).to_bits()
            ]
        ),
        0
    );
    v.mem.write_u32(DATA + 0x100, 1f32.to_bits()).unwrap();
    v.mem.write_u32(DATA + 0x104, 2f32.to_bits()).unwrap();
    assert_eq!(
        gdip(&mut v, "GdipTransformMatrixPoints", &[m, DATA + 0x100, 1]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 12f32.to_bits());
    assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 2f32.to_bits());
    assert_eq!(
        gdip(
            &mut v,
            "GdipVectorTransformMatrixPoints",
            &[m, DATA + 0x100, 1]
        ),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x100).unwrap(), 24f32.to_bits());
    assert_eq!(v.mem.read_u32(DATA + 0x104).unwrap(), 6f32.to_bits());
    v.mem.write_u32(DATA + 0xfff8, 1f32.to_bits()).unwrap();
    v.mem.write_u32(DATA + 0xfffc, 2f32.to_bits()).unwrap();
    assert_eq!(
        gdip(&mut v, "GdipTransformMatrixPoints", &[m, DATA + 0xfff8, 2]),
        2
    );
    assert_eq!(v.mem.read_u32(DATA + 0xfff8).unwrap(), 1f32.to_bits()); // no partial writes across fault
    assert_eq!(
        gdip(&mut v, "GdipTransformMatrixPoints", &[m, DATA + 0x100, 0]),
        2
    );
    assert_eq!(
        gdip(
            &mut v,
            "GdipTransformMatrixPoints",
            &[m, DATA + 0x100, u32::MAX]
        ),
        2
    );
}

#[test]
fn gdiplus_rect_mapping_and_integer_rounding_transform_points() {
    let mut v = Vm::new();
    for (i, n) in [1i32, 2, 4, 8].into_iter().enumerate() {
        v.mem
            .write_u32(DATA + 0x100 + i as u32 * 4, n as u32)
            .unwrap();
    }
    for (i, n) in [10i32, 20, 18, 20, 10, 44].into_iter().enumerate() {
        v.mem
            .write_u32(DATA + 0x200 + i as u32 * 4, n as u32)
            .unwrap();
    }
    assert_eq!(
        gdip(
            &mut v,
            "GdipCreateMatrix3I",
            &[DATA + 0x100, DATA + 0x200, DATA]
        ),
        0
    );
    let m = v.mem.read_u32(DATA).unwrap();
    assert_eq!(matrix_values(&mut v, m), [2., 0., 0., 3., 8., 14.]);
    v.mem.write_u32(DATA + 0x300, 1).unwrap();
    v.mem.write_u32(DATA + 0x304, 2).unwrap();
    assert_eq!(
        gdip(&mut v, "GdipTransformMatrixPointsI", &[m, DATA + 0x300, 1]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), 10);
    assert_eq!(v.mem.read_u32(DATA + 0x304).unwrap(), 20);
    assert_eq!(
        gdip(
            &mut v,
            "GdipSetMatrixElements",
            &[
                m,
                1f32.to_bits(),
                0,
                0,
                1f32.to_bits(),
                0.5f32.to_bits(),
                (-0.5f32).to_bits()
            ]
        ),
        0
    );
    v.mem.write_u32(DATA + 0x300, 0).unwrap();
    v.mem.write_u32(DATA + 0x304, 0).unwrap();
    assert_eq!(
        gdip(&mut v, "GdipTransformMatrixPointsI", &[m, DATA + 0x300, 1]),
        0
    );
    assert_eq!(v.mem.read_u32(DATA + 0x300).unwrap(), 1);
    assert_eq!(v.mem.read_u32(DATA + 0x304).unwrap(), 0);
}
