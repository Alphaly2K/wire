//! Existing behavioral tests grouped by d3d9; expectations remain independent of API catalogs.

use super::*;

fn d3d_device(v: &mut Vm) -> (u32, u32, u32) {
    v.mem
        .write_bytes(CODE, &[0xb8, 1, 0, 0, 0, 0xc2, 16, 0])
        .unwrap();
    let hwnd = test_window(v, CODE);
    let factory = v.call("d3d9.dll", "Direct3DCreate9", &[32]);
    assert_ne!(factory, 0);
    let p = DATA + 0x800;
    for (i, n) in [4, 4, 21, 1, 0, 0, 1, hwnd, 1, 0, 0, 0, 0, 0]
        .into_iter()
        .enumerate()
    {
        v.mem.write_u32(p + i as u32 * 4, n).unwrap();
    }
    assert_eq!(
        d3d_com(v, factory, 16, &[0, 1, hwnd, 0x20, p, DATA + 0x900]),
        0
    );
    let device = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_ne!(device, 0);
    (factory, device, hwnd)
}

#[test]
fn d3d9_target_binding_resets_extent_and_rhw_drives_perspective_sampling() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]);
    let back = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(
        d3d_com(&mut v, device, 28, &[8, 4, 21, 0, 0, 0, DATA + 0x900, 0]),
        0
    );
    let target = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, device, 37, &[0, target]), 0);
    d3d_com(&mut v, device, 48, &[DATA]);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 8);
    assert_eq!(d3d_com(&mut v, device, 37, &[0, back]), 0);
    d3d_com(&mut v, device, 48, &[DATA]);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 4);
    assert_eq!(
        d3d_com(&mut v, device, 23, &[8, 1, 1, 0, 21, 1, DATA + 0x900, 0]),
        0
    );
    let texture = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, texture, 19, &[0, DATA + 0x910, 0, 0]);
    let bits = v.mem.read_u32(DATA + 0x914).unwrap();
    for i in 0..8 {
        v.mem.write_u32(bits + i * 4, 0xff000000 + i * 10).unwrap();
    }
    d3d_com(&mut v, texture, 20, &[0]);
    d3d_com(&mut v, device, 65, &[0, texture]);
    for (i, values) in [
        [0f32, 0., 0.5, 1., 0., 0.],
        [4., 0., 0.5, 0.5, 1., 0.],
        [0., 4., 0.5, 1., 0., 0.],
    ]
    .into_iter()
    .enumerate()
    {
        for (j, n) in values.into_iter().enumerate() {
            v.mem
                .write_u32(DATA + i as u32 * 24 + j as u32 * 4, n.to_bits())
                .unwrap();
        }
    }
    d3d_com(&mut v, device, 89, &[0x104]);
    d3d_com(&mut v, device, 57, &[22, 1]);
    d3d_com(&mut v, device, 41, &[]);
    assert_eq!(d3d_com(&mut v, device, 83, &[4, 1, DATA, 24]), 0);
    d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]);
    let pixels = v.mem.read_u32(DATA + 0x914).unwrap();
    // Barycentrics=(1/2,1/4,1/4), u=(1/4*1/2)/(1/2+1/8+1/4)=1/7.
    assert_eq!(v.mem.read_u32(pixels + 5 * 4).unwrap(), 0xff00000a);
    d3d_com(&mut v, back, 14, &[]);
    d3d_com(&mut v, device, 42, &[]);
    for object in [back, target, texture, device, factory] {
        d3d_com(&mut v, object, 2, &[]);
    }
}

#[test]
fn d3d9_indexed_declarations_transforms_and_reset_preserve_real_resources() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    assert_eq!(
        d3d_com(&mut v, device, 23, &[1, 1, 1, 0, 21, 1, DATA + 0x900, 0]),
        0
    );
    let texture = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, texture, 19, &[0, DATA + 0x910, 0, 0]), 0);
    let texbits = v.mem.read_u32(DATA + 0x914).unwrap();
    v.mem.write_u32(texbits, 0xff336699).unwrap();
    d3d_com(&mut v, texture, 20, &[0]);
    d3d_com(&mut v, device, 65, &[0, texture]);
    d3d_com(&mut v, device, 26, &[140, 0, 0x144, 1, DATA + 0x900, 0]);
    let vb = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, vb, 11, &[0, 0, DATA + 0x910, 0]);
    let vertices = v.mem.read_u32(DATA + 0x910).unwrap();
    for (i, [x, y]) in [[-0.5f32, -0.5], [3.5, -0.5], [-0.5, 3.5], [3.5, 3.5]]
        .into_iter()
        .enumerate()
    {
        let base = vertices + (i as u32 + 1) * 28;
        for (j, n) in [x, y, 0.25, 1.].into_iter().enumerate() {
            v.mem.write_u32(base + j as u32 * 4, n.to_bits()).unwrap();
        }
        v.mem.write_u32(base + 16, 0xffffffff).unwrap();
        v.mem.write_u32(base + 20, 0).unwrap();
        v.mem.write_u32(base + 24, 0).unwrap();
    }
    d3d_com(&mut v, vb, 12, &[]);
    d3d_com(&mut v, device, 27, &[14, 0, 101, 1, DATA + 0x900, 0]);
    let ib = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, ib, 11, &[0, 0, DATA + 0x910, 0]);
    let indices = v.mem.read_u32(DATA + 0x910).unwrap();
    for (i, n) in [999u16, 2, 3, 4, 4, 3, 5].into_iter().enumerate() {
        v.mem.write_u16(indices + i as u32 * 2, n).unwrap();
    }
    d3d_com(&mut v, ib, 12, &[]);
    d3d_com(&mut v, device, 100, &[0, vb, 0, 28]);
    d3d_com(&mut v, device, 104, &[ib]);
    // POSITIONT/COLOR/TEXCOORD declaration owns an exact copy of the elements.
    let elements = [
        0, 0, 0, 0, 3, 0, 9, 0, 0, 0, 16, 0, 4, 0, 10, 0, 0, 0, 20, 0, 1, 0, 5, 0, 255, 0, 0, 0,
        17, 0, 0, 0,
    ];
    v.mem.write_bytes(DATA, &elements).unwrap();
    assert_eq!(d3d_com(&mut v, device, 86, &[DATA, DATA + 0x900]), 0);
    let declaration = v.mem.read_u32(DATA + 0x900).unwrap();
    v.mem.write_bytes(DATA, &[0; 32]).unwrap();
    assert_eq!(d3d_com(&mut v, declaration, 4, &[0, DATA + 0x910]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), 4);
    v.mem.write_u32(DATA + 0x910, 3).unwrap();
    assert_eq!(
        d3d_com(&mut v, declaration, 4, &[DATA, DATA + 0x910]),
        0x88760867
    );
    assert_eq!(d3d_com(&mut v, declaration, 4, &[DATA, DATA + 0x910]), 0);
    let mut copy = [0; 32];
    v.mem.read_bytes(DATA, &mut copy).unwrap();
    assert_eq!(copy, elements);
    d3d_com(&mut v, device, 87, &[declaration]);
    d3d_com(&mut v, device, 57, &[22, 1]);
    d3d_com(&mut v, device, 41, &[]);
    assert_eq!(
        d3d_com(&mut v, device, 82, &[4, (-1i32) as u32, 2, 4, 1, 2]),
        0
    );
    assert_eq!(d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]), 0);
    let back = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]);
    let pixels = v.mem.read_u32(DATA + 0x914).unwrap();
    d3d_com(&mut v, back, 14, &[]);
    for i in 0..16 {
        assert_eq!(v.mem.read_u32(pixels + i * 4).unwrap(), 0xff336699);
    }
    v.mem.write_u16(indices + 2, 1).unwrap();
    assert_eq!(
        d3d_com(&mut v, device, 82, &[4, (-1i32) as u32, 2, 4, 1, 2]),
        0x8876086c
    );
    assert_eq!(v.mem.read_u32(pixels).unwrap(), 0xff336699);
    // Real XYZ vertices transform and clip against all homogeneous planes.
    d3d_com(&mut v, device, 89, &[0x142]);
    d3d_com(&mut v, device, 100, &[0, vb, 28, 24]);
    for (i, [x, y]) in [[-2.5f32, 2.5], [2.5, 2.5], [-2.5, -2.5], [2.5, -2.5]]
        .into_iter()
        .enumerate()
    {
        let base = vertices + 28 + i as u32 * 24;
        for (j, n) in [x, y, 0.25].into_iter().enumerate() {
            v.mem.write_u32(base + j as u32 * 4, n.to_bits()).unwrap();
        }
        v.mem.write_u32(base + 12, 0xffffffff).unwrap();
        v.mem.write_u32(base + 16, 0).unwrap();
        v.mem.write_u32(base + 20, 0).unwrap();
    }
    let matrix = std::array::from_fn::<_, 16, _>(|i| {
        if i == 0 || i == 5 {
            0.5f32
        } else if i % 5 == 0 {
            1.
        } else {
            0.
        }
    });
    for (i, n) in matrix.into_iter().enumerate() {
        v.mem
            .write_u32(DATA + 0x100 + i as u32 * 4, n.to_bits())
            .unwrap();
    }
    assert_eq!(d3d_com(&mut v, device, 44, &[256, DATA + 0x100]), 0);
    assert_eq!(d3d_com(&mut v, device, 45, &[256, DATA + 0x200]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x200).unwrap(), 0.5f32.to_bits());
    d3d_com(&mut v, device, 43, &[0, 0, 1, 0xff000000, 0, 0]);
    assert_eq!(d3d_com(&mut v, device, 81, &[5, 0, 2]), 0);
    for i in 0..16 {
        assert_eq!(v.mem.read_u32(pixels + i * 4).unwrap(), 0xff336699);
    }
    d3d_com(&mut v, device, 42, &[]);
    // A live implicit back-buffer reference prevents Reset without changing it.
    let params = [8u32, 4, 21, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0];
    for (i, n) in params.into_iter().enumerate() {
        v.mem.write_u32(DATA + 0x800 + i as u32 * 4, n).unwrap();
    }
    assert_eq!(d3d_com(&mut v, device, 16, &[DATA + 0x800]), 0x8876086c);
    assert_eq!(v.mem.read_u32(pixels).unwrap(), 0xff336699);
    d3d_com(&mut v, back, 2, &[]);
    assert_eq!(d3d_com(&mut v, device, 16, &[DATA + 0x800]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x800).unwrap(), 8);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, pixels]),
        u32::MAX
    );
    // Managed buffer/texture/declaration storage survives and bindings reset.
    assert_ne!(
        v.k("HeapSize", &[v.state.process_heap, 0, vertices]),
        u32::MAX
    );
    assert_eq!(v.mem.read_u32(texbits).unwrap(), 0xff336699);
    assert_eq!(d3d_com(&mut v, device, 88, &[DATA + 0x910]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), 0);
    assert_eq!(d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]), 0);
    let back = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, back, 12, &[DATA + 0x100]);
    assert_eq!(v.mem.read_u32(DATA + 0x118).unwrap(), 8);
    for object in [back, declaration, ib, vb, texture, device, factory] {
        d3d_com(&mut v, object, 2, &[]);
    }
}

#[test]
fn d3d9_depth_stencil_alpha_and_vertex_streams_affect_real_pixels() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    assert_eq!(d3d_com(&mut v, factory, 10, &[0, 1, 22, 2, 1, 75]), 0);
    assert_eq!(
        d3d_com(&mut v, device, 29, &[4, 4, 75, 0, 0, 0, DATA + 0x900, 0]),
        0
    );
    let depth = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, device, 39, &[depth]), 0);
    assert_eq!(
        d3d_com(&mut v, depth, 13, &[DATA + 0x910, 0, 0]),
        0x8876086a
    );
    assert_eq!(d3d_com(&mut v, device, 40, &[DATA + 0x910]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), depth);
    assert_eq!(d3d_com(&mut v, depth, 2, &[]), 1);
    assert_eq!(
        d3d_com(
            &mut v,
            device,
            43,
            &[0, 0, 7, 0xff112233, 1f32.to_bits(), 3]
        ),
        0
    );
    assert_eq!(
        d3d_com(&mut v, device, 23, &[1, 1, 1, 0, 21, 1, DATA + 0x900, 0]),
        0
    );
    let texture = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, texture, 19, &[0, DATA + 0x910, 0, 0]), 0);
    let texbits = v.mem.read_u32(DATA + 0x914).unwrap();
    v.mem.write_u32(texbits, 0xffff0000).unwrap();
    assert_eq!(d3d_com(&mut v, texture, 20, &[0]), 0);
    assert_eq!(d3d_com(&mut v, device, 65, &[0, texture]), 0);
    assert_eq!(
        d3d_com(&mut v, device, 26, &[96, 0, 0x104, 1, DATA + 0x900, 0]),
        0
    );
    let buffer = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, buffer, 11, &[0, 0, DATA + 0x910, 0]), 0);
    let vertices = v.mem.read_u32(DATA + 0x910).unwrap();
    assert_eq!(
        d3d_com(&mut v, buffer, 11, &[0, 4, DATA + 0x910, 0]),
        0x8876086c
    );
    let points = [[-0.5f32, -0.5], [3.5, -0.5], [-0.5, 3.5], [3.5, 3.5]];
    for (i, [x, y]) in points.into_iter().enumerate() {
        for (j, n) in [x, y, 0.25, 1., 0., 0.].into_iter().enumerate() {
            v.mem
                .write_u32(vertices + i as u32 * 24 + j as u32 * 4, n.to_bits())
                .unwrap();
        }
    }
    assert_eq!(d3d_com(&mut v, buffer, 12, &[]), 0);
    assert_eq!(
        d3d_com(&mut v, buffer, 11, &[95, 2, DATA + 0x910, 0]),
        0x8876086c
    );
    assert_eq!(d3d_com(&mut v, device, 100, &[0, buffer, 0, 24]), 0);
    assert_eq!(d3d_com(&mut v, device, 89, &[0x104]), 0);
    for (state, value) in [(22, 1), (7, 1), (52, 1), (56, 3), (57, 3), (55, 4)] {
        assert_eq!(d3d_com(&mut v, device, 57, &[state, value]), 0);
    }
    assert_eq!(d3d_com(&mut v, device, 41, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 81, &[5, 0, 2]), 0);
    assert_eq!(d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x920]), 0);
    let back = v.mem.read_u32(DATA + 0x920).unwrap();
    assert_eq!(d3d_com(&mut v, back, 13, &[DATA + 0x930, 0, 0x10]), 0);
    let pixels = v.mem.read_u32(DATA + 0x934).unwrap();
    assert_eq!(v.mem.read_u32(pixels).unwrap(), 0xffff0000);
    assert_eq!(d3d_com(&mut v, back, 14, &[]), 0);
    // Stencil was incremented to four: the second red draw must be rejected.
    assert_eq!(d3d_com(&mut v, device, 43, &[0, 0, 1, 0xff00ff00, 0, 0]), 0);
    assert_eq!(d3d_com(&mut v, device, 81, &[5, 0, 2]), 0);
    assert_eq!(v.mem.read_u32(pixels).unwrap(), 0xff00ff00);
    // Stencil disabled; farther z fails, even though the triangle covers pixel zero.
    assert_eq!(d3d_com(&mut v, device, 57, &[52, 0]), 0);
    for i in 0..4 {
        v.mem
            .write_u32(vertices + i * 24 + 8, 0.75f32.to_bits())
            .unwrap();
    }
    assert_eq!(d3d_com(&mut v, device, 81, &[5, 0, 2]), 0);
    assert_eq!(v.mem.read_u32(pixels).unwrap(), 0xff00ff00);
    assert_eq!(d3d_com(&mut v, device, 57, &[7, 0]), 0);
    // Alpha test discards zero-alpha samples before updating any depth/stencil.
    v.mem.write_u32(texbits, 0x00ffffff).unwrap();
    for (state, value) in [(15, 1), (25, 5), (24, 0)] {
        assert_eq!(d3d_com(&mut v, device, 57, &[state, value]), 0);
    }
    assert_eq!(d3d_com(&mut v, device, 81, &[5, 0, 2]), 0);
    assert_eq!(v.mem.read_u32(pixels).unwrap(), 0xff00ff00);
    assert_eq!(d3d_com(&mut v, device, 42, &[]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 2, &[]), 0);
    assert_ne!(
        v.k("HeapSize", &[v.state.process_heap, 0, vertices]),
        u32::MAX
    );
    assert_eq!(d3d_com(&mut v, back, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, depth, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, texture, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 2, &[]), 0);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, vertices]),
        u32::MAX
    );
    assert_eq!(d3d_com(&mut v, factory, 2, &[]), 0);
}

#[test]
fn d3d9_com_texture_quad_present_has_real_pixels_and_releases_bound_resources() {
    let mut v = Vm::new();
    let (factory, device, hwnd) = d3d_device(&mut v);
    assert_eq!(
        d3d_com(&mut v, device, 23, &[2, 2, 1, 0, 21, 1, DATA + 0x900, 0]),
        0
    );
    let texture = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, texture, 19, &[0, DATA + 0x910, 0, 0]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), 8);
    let bits = v.mem.read_u32(DATA + 0x914).unwrap();
    for (i, n) in [0xffff0000, 0xff00ff00, 0xff0000ff, 0xffffffff]
        .into_iter()
        .enumerate()
    {
        v.mem.write_u32(bits + i as u32 * 4, n).unwrap();
    }
    assert_eq!(d3d_com(&mut v, texture, 20, &[0]), 0);
    assert_eq!(d3d_com(&mut v, device, 65, &[0, texture]), 0);
    assert_eq!(d3d_com(&mut v, device, 89, &[0x104]), 0);
    assert_eq!(d3d_com(&mut v, device, 57, &[22, 1]), 0);
    for (i, vertex) in [
        [-0.5f32, -0.5, 0.5, 1., 0., 0.],
        [3.5, -0.5, 0.5, 1., 1., 0.],
        [-0.5, 3.5, 0.5, 1., 0., 1.],
        [3.5, 3.5, 0.5, 1., 1., 1.],
    ]
    .into_iter()
    .enumerate()
    {
        for (j, n) in vertex.into_iter().enumerate() {
            v.mem
                .write_u32(DATA + 0xa00 + i as u32 * 24 + j as u32 * 4, n.to_bits())
                .unwrap();
        }
    }
    assert_eq!(d3d_com(&mut v, device, 41, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 83, &[5, 2, DATA + 0xa00, 24]), 0);
    assert_eq!(d3d_com(&mut v, device, 42, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 17, &[0, 0, 0, 0]), 0);
    let (w, h, pixels) = v.state.graphics.window_snapshot(&v.mem, hwnd).unwrap();
    assert_eq!((w, h), (640, 480));
    assert_eq!(pixels[0], 0xff);
    assert_eq!(pixels[639], 0xff00);
    assert_eq!(pixels[479 * 640], 0xff0000);
    assert_eq!(pixels[480 * 640 - 1], 0xffffff);
    assert_eq!(v.state.d3d9.frames, 1);
    assert_eq!(v.state.d3d9.draws, 1);
    // Device owns the binding after the guest drops its COM reference.
    assert_eq!(d3d_com(&mut v, texture, 2, &[]), 0);
    assert_ne!(v.k("HeapSize", &[v.state.process_heap, 0, bits]), u32::MAX);
    assert_eq!(d3d_com(&mut v, factory, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 2, &[]), 0);
    assert_eq!(v.k("HeapSize", &[v.state.process_heap, 0, bits]), u32::MAX);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, texture]),
        u32::MAX
    );
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, device]),
        u32::MAX
    );
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, factory]),
        u32::MAX
    );
}

#[test]
fn d3d9_surface_locks_and_external_child_references_preserve_com_owners() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    assert_eq!(d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]), 0);
    let surface = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, surface, 13, &[DATA + 0x910, 0, 0]), 0);
    let bits = v.mem.read_u32(DATA + 0x914).unwrap();
    assert_eq!(
        d3d_com(&mut v, surface, 13, &[DATA + 0x920, 0, 0]),
        0x8876086c
    );
    assert_eq!(
        d3d_com(&mut v, device, 43, &[0, 0, 1, 0xff123456, 0, 0]),
        0x8876086c
    );
    assert_eq!(d3d_com(&mut v, surface, 14, &[]), 0);
    assert_eq!(d3d_com(&mut v, surface, 14, &[]), 0x8876086c);
    assert_eq!(d3d_com(&mut v, device, 43, &[0, 0, 1, 0xff123456, 0, 0]), 0);
    assert_eq!(v.mem.read_u32(bits).unwrap(), 0xff123456);
    assert_eq!(d3d_com(&mut v, device, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, surface, 3, &[DATA + 0x920]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x920).unwrap(), device);
    assert_eq!(d3d_com(&mut v, device, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, surface, 2, &[]), 0);
    assert_eq!(v.k("HeapSize", &[v.state.process_heap, 0, bits]), u32::MAX);
    assert_eq!(d3d_com(&mut v, factory, 2, &[]), 0);
}

#[test]
fn d3d9_failed_outputs_formats_and_vertex_reads_leave_pixels_unchanged() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    let available = d3d_com(&mut v, device, 4, &[]);
    assert_eq!(
        d3d_com(&mut v, device, 23, &[2, 2, 1, 0, 21, 1, 0x30000000, 0]),
        0x8876086c
    );
    assert_eq!(
        d3d_com(&mut v, device, 23, &[2, 2, 1, 0, 23, 1, DATA + 0x900, 0]),
        0x8876086a
    );
    assert_eq!(v.mem.read_u32(DATA + 0x900).unwrap(), 0);
    assert_eq!(d3d_com(&mut v, device, 4, &[]), available);
    assert_eq!(d3d_com(&mut v, device, 89, &[0x104]), 0);
    assert_eq!(d3d_com(&mut v, device, 41, &[]), 0);
    assert_eq!(
        d3d_com(&mut v, device, 83, &[5, 2, 0x30000000, 24]),
        0x8876086c
    );
    assert_eq!(v.state.d3d9.draws, 0);
    assert_eq!(d3d_com(&mut v, device, 42, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, factory, 2, &[]), 0);
}

#[test]
fn d3d9_alpha_quad_shared_edges_scissor_and_diffuse_vertices_are_exact() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    assert_eq!(
        d3d_com(&mut v, device, 23, &[1, 1, 1, 0, 21, 1, DATA + 0x900, 0]),
        0
    );
    let texture = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(
        d3d_com(&mut v, texture, 19, &[0, DATA + 0x910, 0, 0x8000]),
        0
    );
    let bits = v.mem.read_u32(DATA + 0x914).unwrap();
    v.mem.write_u32(bits, 0x80ffffff).unwrap();
    assert_eq!(d3d_com(&mut v, texture, 20, &[0]), 0);
    assert_eq!(d3d_com(&mut v, device, 65, &[0, texture]), 0);
    assert_eq!(d3d_com(&mut v, device, 89, &[0x144]), 0);
    for (n, value) in [(22, 1), (27, 1), (19, 5), (20, 6), (174, 1)] {
        assert_eq!(d3d_com(&mut v, device, 57, &[n, value]), 0);
    }
    for (i, n) in [1, 1, 3, 3].into_iter().enumerate() {
        v.mem.write_u32(DATA + 0x940 + i as u32 * 4, n).unwrap();
    }
    assert_eq!(d3d_com(&mut v, device, 75, &[DATA + 0x940]), 0);
    assert_eq!(d3d_com(&mut v, device, 43, &[0, 0, 1, 0xff0000ff, 0, 0]), 0);
    for (i, [x, y, u, t]) in [
        [-0.5f32, -0.5, 0., 0.],
        [3.5, -0.5, 1., 0.],
        [-0.5, 3.5, 0., 1.],
        [3.5, 3.5, 1., 1.],
    ]
    .into_iter()
    .enumerate()
    {
        let words = [
            x.to_bits(),
            y.to_bits(),
            1f32.to_bits(),
            1f32.to_bits(),
            0xffffffff,
            u.to_bits(),
            t.to_bits(),
        ];
        for (j, n) in words.into_iter().enumerate() {
            v.mem
                .write_u32(DATA + 0xa00 + i as u32 * 28 + j as u32 * 4, n)
                .unwrap();
        }
    }
    assert_eq!(d3d_com(&mut v, device, 41, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 83, &[5, 2, DATA + 0xa00, 28]), 0);
    assert_eq!(d3d_com(&mut v, device, 42, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]), 0);
    let back = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]), 0);
    let pixels = v.mem.read_u32(DATA + 0x914).unwrap();
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(
                v.mem.read_u32(pixels + (y * 4 + x) * 4).unwrap(),
                if (1..3).contains(&x) && (1..3).contains(&y) {
                    0xbf8080ff
                } else {
                    0xff0000ff
                }
            );
        }
    }
    d3d_com(&mut v, back, 14, &[]);
    d3d_com(&mut v, back, 2, &[]);
    d3d_com(&mut v, texture, 2, &[]);
    d3d_com(&mut v, device, 2, &[]);
    d3d_com(&mut v, factory, 2, &[]);
}

#[test]
fn d3d9_pixel_shader_executes_constants_masks_and_preserves_bound_lifetime() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    // def c0, (0.25,0.5,0.75,1); mov oC0,c0
    let tokens = [
        0xffff0200,
        0x05000051,
        0xa00f0000,
        0.25f32.to_bits(),
        0.5f32.to_bits(),
        0.75f32.to_bits(),
        1f32.to_bits(),
        0x02000001,
        0x800f0800,
        0xa0e40000,
        0xffff,
    ];
    let p = DATA + 0x200;
    v.mem
        .write_bytes(
            p,
            &tokens
                .iter()
                .flat_map(|n| n.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, device, 106, &[p, DATA + 0x900]), 0);
    let shader = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, shader, 4, &[0, DATA + 0x910]), 0);
    assert_eq!(
        v.mem.read_u32(DATA + 0x910).unwrap(),
        tokens.len() as u32 * 4
    );
    v.mem.write_u32(DATA + 0x910, 4).unwrap();
    assert_eq!(
        d3d_com(&mut v, shader, 4, &[DATA + 0x300, DATA + 0x910]),
        0x88760867
    );
    assert_eq!(d3d_com(&mut v, shader, 4, &[DATA + 0x300, DATA + 0x910]), 0);
    let mut copy = vec![0; tokens.len() * 4];
    v.mem.read_bytes(DATA + 0x300, &mut copy).unwrap();
    assert_eq!(
        copy,
        tokens
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect::<Vec<_>>()
    );
    assert_eq!(d3d_com(&mut v, device, 107, &[shader]), 0);
    assert_eq!(d3d_com(&mut v, shader, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 108, &[DATA + 0x900]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x900).unwrap(), shader);
    assert_eq!(d3d_com(&mut v, shader, 2, &[]), 0);
    // Constants set atomically, local def shadows device c0.
    let c = [0f32, 0., 0., 0.];
    v.mem
        .write_bytes(
            DATA + 0x400,
            &c.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, device, 109, &[0, DATA + 0x400, 1]), 0);
    assert_eq!(
        d3d_com(&mut v, device, 109, &[31, DATA + 0x400, 2]),
        0x8876086c
    );
    assert_eq!(d3d_com(&mut v, device, 110, &[0, DATA + 0x410, 1]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x410).unwrap(), 0);
    for (i, values) in [
        [0f32, 0., 0.5, 1., 0., 0.],
        [4., 0., 0.5, 1., 1., 0.],
        [0., 4., 0.5, 1., 0., 1.],
    ]
    .into_iter()
    .enumerate()
    {
        v.mem
            .write_bytes(
                DATA + i as u32 * 24,
                &values
                    .iter()
                    .flat_map(|n| n.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    assert_eq!(d3d_com(&mut v, device, 89, &[0x104]), 0);
    d3d_com(&mut v, device, 57, &[22, 1]);
    d3d_com(&mut v, device, 57, &[168, 5]); // only R and B
    d3d_com(
        &mut v,
        device,
        43,
        &[0, 0, 1, 0xff123456, 1f32.to_bits(), 0],
    );
    d3d_com(&mut v, device, 41, &[]);
    assert_eq!(d3d_com(&mut v, device, 83, &[4, 1, DATA, 24]), 0);
    d3d_com(&mut v, device, 42, &[]);
    d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]);
    let back = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]);
    let bits = v.mem.read_u32(DATA + 0x914).unwrap();
    assert_eq!(v.mem.read_u32(bits).unwrap(), 0xff4034bf);
    d3d_com(&mut v, back, 14, &[]);
    assert_eq!(d3d_com(&mut v, device, 107, &[0]), 0);
    // Malformed stream cannot publish an object.
    v.mem.write_u32(p + 4, 0x04000001).unwrap();
    assert_ne!(d3d_com(&mut v, device, 106, &[p, DATA + 0x900]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x900).unwrap(), 0);
    for o in [back, device, factory] {
        d3d_com(&mut v, o, 2, &[]);
    }
}

#[test]
fn d3d9_recorded_stateblocks_isolate_live_state_capture_only_members_and_hold_resources() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    assert_eq!(d3d_com(&mut v, device, 61, &[DATA + 0x900]), 0x8876086c);
    assert_eq!(d3d_com(&mut v, device, 60, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 60, &[]), 0x8876086c);
    assert_eq!(d3d_com(&mut v, device, 59, &[1, DATA + 0x900]), 0x8876086c);
    assert_eq!(d3d_com(&mut v, device, 57, &[22, 1]), 0);
    assert_eq!(d3d_com(&mut v, device, 57, &[168, 3]), 0);
    assert_eq!(d3d_com(&mut v, device, 57, &[168, 16]), 0x8876086a);
    assert_eq!(d3d_com(&mut v, device, 58, &[22, DATA]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 3); // getter sees live state
    assert_eq!(
        d3d_com(&mut v, device, 23, &[1, 1, 1, 0, 21, 1, DATA + 0x900, 0]),
        0
    );
    let texture = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, device, 65, &[1, texture]), 0);
    assert_eq!(d3d_com(&mut v, texture, 2, &[]), 0); // recording owns the resource
    assert_eq!(d3d_com(&mut v, device, 64, &[1, DATA + 0x900]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x900).unwrap(), 0);
    assert_eq!(d3d_com(&mut v, device, 61, &[0]), 0x8876086c); // keep recording on bad output
    assert_eq!(d3d_com(&mut v, device, 61, &[DATA + 0x900]), 0);
    let block = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, device, 57, &[27, 1]); // unrecorded blend-enable
    assert_eq!(d3d_com(&mut v, block, 5, &[]), 0);
    d3d_com(&mut v, device, 58, &[22, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    d3d_com(&mut v, device, 58, &[168, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 3);
    d3d_com(&mut v, device, 58, &[27, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    d3d_com(&mut v, device, 64, &[1, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), texture);
    d3d_com(&mut v, texture, 2, &[]);
    d3d_com(&mut v, device, 57, &[22, 2]);
    assert_eq!(d3d_com(&mut v, block, 4, &[]), 0);
    d3d_com(&mut v, device, 57, &[22, 3]);
    assert_eq!(d3d_com(&mut v, block, 5, &[]), 0);
    d3d_com(&mut v, device, 58, &[22, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 2);
    assert_eq!(d3d_com(&mut v, block, 3, &[DATA]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), device);
    d3d_com(&mut v, device, 2, &[]);
    assert_eq!(d3d_com(&mut v, block, 2, &[]), 0);
    assert_eq!(d3d_com(&mut v, device, 64, &[1, DATA]), 0);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), texture); // live binding survives block release
    d3d_com(&mut v, texture, 2, &[]);
    d3d_com(&mut v, device, 65, &[1, 0]);
    assert_eq!(
        v.call("d3d9.dll", "IDirect3DTexture9.AddRef", &[texture]),
        0x8876086c
    );
    for o in [device, factory] {
        d3d_com(&mut v, o, 2, &[]);
    }
}

#[test]
fn d3d9_all_stateblocks_restore_defaults_constants_and_exclude_render_targets() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    assert_eq!(d3d_com(&mut v, device, 59, &[1, DATA + 0x900]), 0);
    let block = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(
        d3d_com(&mut v, device, 28, &[8, 4, 21, 0, 0, 0, DATA + 0x900, 0]),
        0
    );
    let target = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, device, 37, &[0, target]);
    d3d_com(&mut v, device, 57, &[22, 1]);
    d3d_com(&mut v, device, 69, &[1, 3, 3]); // address W is stored for 2D textures
    v.mem.write_bytes(DATA, &[0x55; 16]).unwrap();
    d3d_com(&mut v, device, 109, &[5, DATA, 1]);
    assert_eq!(d3d_com(&mut v, block, 5, &[]), 0);
    d3d_com(&mut v, device, 38, &[0, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), target);
    d3d_com(&mut v, target, 2, &[]);
    d3d_com(&mut v, device, 48, &[DATA]);
    assert_eq!(v.mem.read_u32(DATA + 8).unwrap(), 4); // saved viewport, not target extent
    d3d_com(&mut v, device, 58, &[22, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 3);
    d3d_com(&mut v, device, 68, &[1, 3, DATA]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 1);
    d3d_com(&mut v, device, 110, &[5, DATA, 1]);
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0);
    assert_eq!(d3d_com(&mut v, device, 59, &[0, DATA]), 0x8876086c);
    for o in [block, target, device, factory] {
        d3d_com(&mut v, o, 2, &[]);
    }
}

#[test]
fn d3d9_vs2_dp4_constants_clip_position_and_forward_color_to_ps2() {
    let mut v = Vm::new();
    let (factory, device, _) = d3d_device(&mut v);
    let tokens = [
        0xfffe0200u32,
        0x0200001f,
        0x80000000,
        0x900f0000,
        0x0200001f,
        0x80000005,
        0x900f0001,
        0x0200001f,
        0x8000000a,
        0x900f0002,
        0x03000009,
        0xc0010000,
        0x90e40000,
        0xa0e40000,
        0x03000009,
        0xc0020000,
        0x90e40000,
        0xa0e40001,
        0x03000009,
        0xc0040000,
        0x90e40000,
        0xa0e40002,
        0x03000009,
        0xc0080000,
        0x90e40000,
        0xa0e40003,
        0x02000001,
        0xe0030000,
        0x90e40001,
        0x02000001,
        0xd00f0000,
        0x90e40002,
        0xffff,
    ];
    let vp = [0u32, 0, 8, 8, 0, 1f32.to_bits()];
    v.mem
        .write_bytes(
            DATA + 0x800,
            &vp.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, device, 47, &[DATA + 0x800]), 0);
    v.mem.write_u32(DATA + 0x808, 0).unwrap();
    v.mem.write_u32(DATA + 0x80c, 0).unwrap();
    assert_eq!(d3d_com(&mut v, device, 47, &[DATA + 0x800]), 0);
    assert_eq!(d3d_com(&mut v, device, 48, &[DATA + 0x800]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x808).unwrap(), 0);
    v.mem.write_u32(DATA + 0x808, 4).unwrap();
    v.mem.write_u32(DATA + 0x80c, 4).unwrap();
    assert_eq!(d3d_com(&mut v, device, 47, &[DATA + 0x800]), 0);
    let p = DATA + 0x200;
    v.mem
        .write_bytes(
            p,
            &tokens
                .iter()
                .flat_map(|n| n.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, device, 91, &[p, DATA + 0x900]), 0);
    let vs = v.mem.read_u32(DATA + 0x900).unwrap();
    let decl = [
        0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 16, 0, 1, 0, 5, 0, 0, 0, 24, 0, 4, 0, 10, 0, 255, 0, 0, 0,
        17, 0, 0, 0,
    ];
    v.mem.write_bytes(DATA + 0x600, &decl).unwrap();
    assert_eq!(
        d3d_com(&mut v, device, 86, &[DATA + 0x600, DATA + 0x900]),
        0
    );
    let declaration = v.mem.read_u32(DATA + 0x900).unwrap();
    assert_eq!(d3d_com(&mut v, device, 87, &[declaration]), 0);
    assert_eq!(d3d_com(&mut v, device, 92, &[vs]), 0);
    let matrix = [
        0.5f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    v.mem
        .write_bytes(
            DATA + 0x400,
            &matrix
                .iter()
                .flat_map(|n| n.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, device, 94, &[0, DATA + 0x400, 4]), 0);
    assert_eq!(
        d3d_com(&mut v, device, 94, &[255, DATA + 0x400, 2]),
        0x8876086c
    );
    let ps = [
        0xffff0200u32,
        0x0200001f,
        0x80000000,
        0x900f0000,
        0x02000001,
        0x800f0800,
        0x90e40000,
        0xffff,
    ];
    v.mem
        .write_bytes(
            DATA + 0x700,
            &ps.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        d3d_com(&mut v, device, 106, &[DATA + 0x700, DATA + 0x900]),
        0
    );
    let shader = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, device, 107, &[shader]);
    for (i, point) in [
        [-1f32, 1., 0.5, 1., 0., 0.],
        [1., 1., 0.5, 1., 1., 0.],
        [-1., -1., 0.5, 1., 0., 1.],
    ]
    .into_iter()
    .enumerate()
    {
        v.mem
            .write_bytes(
                DATA + i as u32 * 28,
                &point
                    .iter()
                    .flat_map(|n| n.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        v.mem
            .write_u32(DATA + i as u32 * 28 + 24, 0xff4080bf)
            .unwrap();
    }
    d3d_com(&mut v, device, 43, &[0, 0, 1, 0, 1f32.to_bits(), 0]);
    d3d_com(&mut v, device, 41, &[]);
    assert_eq!(d3d_com(&mut v, device, 83, &[4, 1, DATA, 28]), 0);
    d3d_com(&mut v, device, 42, &[]);
    d3d_com(&mut v, device, 18, &[0, 0, 0, DATA + 0x900]);
    let back = v.mem.read_u32(DATA + 0x900).unwrap();
    d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]);
    let bits = v.mem.read_u32(DATA + 0x914).unwrap();
    assert_eq!(v.mem.read_u32(bits + 5 * 4).unwrap(), 0xff4080bf);
    assert_eq!(v.mem.read_u32(bits + 4 * 4).unwrap(), 0);
    d3d_com(&mut v, back, 14, &[]);
    // Switching to pretransformed input bypasses the still-bound VS. Its
    // matrix would collapse these screen coordinates if it were executed.
    assert_eq!(d3d_com(&mut v, device, 89, &[0x144]), 0);
    d3d_com(&mut v, device, 57, &[22, 1]);
    for (i, xy) in [[0f32, 0.], [4., 0.], [0., 4.]].into_iter().enumerate() {
        let p = DATA + i as u32 * 28;
        for (j, n) in [xy[0], xy[1], 0.5, 1.].into_iter().enumerate() {
            v.mem.write_u32(p + j as u32 * 4, n.to_bits()).unwrap();
        }
        v.mem.write_u32(p + 16, 0xffc04080).unwrap();
        v.mem.write_u32(p + 20, 0).unwrap();
        v.mem.write_u32(p + 24, 0).unwrap();
    }
    assert_eq!(d3d_com(&mut v, device, 69, &[0, 7, 1]), 0);
    assert_eq!(d3d_com(&mut v, device, 69, &[0, 7, 2]), 0);
    assert_eq!(d3d_com(&mut v, device, 68, &[0, 7, DATA + 0x920]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x920).unwrap(), 2);
    d3d_com(&mut v, device, 43, &[0, 0, 1, 0, 1f32.to_bits(), 0]);
    d3d_com(&mut v, device, 41, &[]);
    assert_eq!(d3d_com(&mut v, device, 83, &[4, 1, DATA, 28]), 0);
    d3d_com(&mut v, device, 42, &[]);
    d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]);
    assert_eq!(v.mem.read_u32(bits + 5 * 4).unwrap(), 0xffc04080);
    d3d_com(&mut v, back, 14, &[]);
    assert_eq!(d3d_com(&mut v, device, 93, &[DATA + 0x920]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x920).unwrap(), vs);
    d3d_com(&mut v, vs, 2, &[]);
    d3d_com(&mut v, device, 107, &[0]);
    // Null stage-zero texture disables the default MODULATE stage, retaining
    // diffuse RGBA instead of failing or sampling a fabricated texture.
    assert_eq!(d3d_com(&mut v, device, 69, &[0, 12, 0]), 0);
    assert_eq!(d3d_com(&mut v, device, 69, &[0, 13, 0]), 0);
    assert_eq!(d3d_com(&mut v, device, 69, &[0, 12, 1]), 0x8876086a);
    d3d_com(&mut v, device, 43, &[0, 0, 1, 0, 1f32.to_bits(), 0]);
    d3d_com(&mut v, device, 41, &[]);
    assert_eq!(d3d_com(&mut v, device, 83, &[4, 1, DATA, 28]), 0);
    d3d_com(&mut v, device, 42, &[]);
    d3d_com(&mut v, back, 13, &[DATA + 0x910, 0, 0x10]);
    assert_eq!(v.mem.read_u32(bits + 5 * 4).unwrap(), 0xffc04080);
    d3d_com(&mut v, back, 14, &[]);
    d3d_com(&mut v, device, 92, &[0]);
    d3d_com(&mut v, device, 87, &[0]);
    for o in [back, shader, vs, declaration, device, factory] {
        d3d_com(&mut v, o, 2, &[]);
    }
}
