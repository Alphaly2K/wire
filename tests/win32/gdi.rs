//! Existing behavioral tests grouped by gdi; expectations remain independent of API catalogs.

use super::*;

fn dib32(v: &mut Vm, dc: u32, width: u32, height: i32) -> (u32, u32) {
    let bytes: Vec<_> = [40, width, height as u32, 0x00200001, 0, 0, 0, 0, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    v.mem.write_bytes(DATA + 0xd00, &bytes).unwrap();
    let h = v.call(
        "gdi32.dll",
        "CreateDIBSection",
        &[dc, DATA + 0xd00, 0, DATA + 0xd40, 0, 0],
    );
    assert_ne!(h, 0);
    (h, v.mem.read_u32(DATA + 0xd40).unwrap())
}

#[test]
fn logical_fonts_preserve_logfont_encoding_selection_and_14_argument_creation() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    let stock = v.call("gdi32.dll", "GetStockObject", &[17]);
    assert_ne!(stock, 0);
    assert_eq!(v.call("gdi32.dll", "GetObjectW", &[stock, 92, DATA]), 92);
    assert_eq!(v.mem.read_u32(DATA).unwrap() as i32, -16);
    v.wide(DATA + 0x400, "日本語font");
    let font = v.call(
        "gdi32.dll",
        "CreateFontW",
        &[
            (-20i32) as u32,
            0,
            0,
            0,
            700,
            1,
            0,
            0,
            128,
            0,
            0,
            4,
            0,
            DATA + 0x400,
        ],
    );
    assert_ne!(font, 0);
    assert_eq!(v.call("gdi32.dll", "GetObjectType", &[font]), 6);
    assert_eq!(v.call("gdi32.dll", "GetObjectW", &[font, 92, DATA]), 92);
    assert_eq!(v.mem.read_u32(DATA + 16).unwrap(), 700);
    assert_eq!(v.mem.read_wstr(DATA + 28, 32).unwrap(), "日本語font");
    let old = v.call("gdi32.dll", "SelectObject", &[dc, font]);
    assert_ne!(old, 0);
    assert_eq!(v.call("gdi32.dll", "DeleteObject", &[font]), 0);
    assert_eq!(v.call("gdi32.dll", "GetCurrentObject", &[dc, 6]), font);
    v.call("gdi32.dll", "SelectObject", &[dc, old]);
    assert_eq!(v.call("gdi32.dll", "DeleteObject", &[font]), 1);
    assert_eq!(v.call("gdi32.dll", "DeleteObject", &[stock]), 1);
    assert_eq!(v.call("gdi32.dll", "GetObjectA", &[stock, 60, DATA]), 60);
    let a = v.call("gdi32.dll", "CreateFontIndirectA", &[DATA]);
    assert_ne!(a, 0);
    assert_eq!(
        v.call("gdi32.dll", "GetObjectW", &[a, 92, DATA + 0x100]),
        92
    );
    assert_eq!(
        v.mem.read_wstr(DATA + 0x100 + 28, 32).unwrap(),
        "MS UI Gothic"
    );
}

#[test]
fn dib_stretch_and_readback_use_real_bgra_rows_and_checked_scan_ranges() {
    let mut v = Vm::new();
    let screen = v.call("user32.dll", "GetDC", &[0]);
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[screen]);
    let bitmap = v.call("gdi32.dll", "CreateCompatibleBitmap", &[screen, 4, 4]);
    let old = v.call("gdi32.dll", "SelectObject", &[dc, bitmap]);
    for (i, n) in [40, 2, (-2i32) as u32, 1 | (32 << 16), 0, 16, 0, 0, 0, 0]
        .into_iter()
        .enumerate()
    {
        v.mem.write_u32(DATA + i as u32 * 4, n).unwrap();
    }
    for (i, n) in [0xff0000, 0xff00, 0xff, 0xffffff].into_iter().enumerate() {
        v.mem.write_u32(DATA + 0x100 + i as u32 * 4, n).unwrap();
    }
    assert_eq!(
        v.call(
            "gdi32.dll",
            "StretchDIBits",
            &[dc, 0, 0, 4, 4, 0, 0, 2, 2, DATA + 0x100, DATA, 0, 0xcc0020]
        ),
        2
    );
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), 0xff);
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 3, 3]), 0xffffff);
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 3]), 0xff0000);
    assert_eq!(
        v.call(
            "gdi32.dll",
            "StretchDIBits",
            &[dc, 0, 0, 4, 4, 0, 0, 2, 2, 0, DATA, 0, 0xcc0020]
        ),
        0
    );
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), 0xff);
    v.call("gdi32.dll", "SelectObject", &[dc, old]);
    assert_eq!(
        v.call("gdi32.dll", "GetDIBits", &[dc, bitmap, 0, 4, 0, DATA, 0]),
        4
    );
    assert_eq!(v.mem.read_u32(DATA + 4).unwrap(), 4);
    assert_eq!(
        v.call(
            "gdi32.dll",
            "GetDIBits",
            &[dc, bitmap, 0, 4, DATA + 0x200, DATA, 0]
        ),
        4
    );
    assert_eq!(v.mem.read_u32(DATA + 0x200).unwrap(), 0xff); // bottom row: blue
    assert_eq!(v.mem.read_u32(DATA + 0x200 + 3 * 16).unwrap(), 0xff0000); // top row: red
    assert_eq!(
        v.call(
            "gdi32.dll",
            "GetDIBits",
            &[dc, bitmap, 4, 1, DATA + 0x200, DATA, 0]
        ),
        0
    );
}

#[test]
fn gdi_color_probe_dc_bitmap_lifetimes_and_pixel_failures_are_real() {
    let mut v = Vm::new();
    let screen = v.call("user32.dll", "GetDC", &[0]);
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[screen]);
    let bitmap = v.call("gdi32.dll", "CreateCompatibleBitmap", &[screen, 1, 1]);
    assert_ne!(screen, 0);
    assert_ne!(bitmap, 0);
    let stock = v.call("gdi32.dll", "SelectObject", &[dc, bitmap]);
    assert_ne!(stock, 0);
    for component in 0..3 {
        for value in 0..256 {
            let color = value << (component * 8);
            assert_eq!(v.call("gdi32.dll", "SetPixel", &[dc, 0, 0, color]), color);
            assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), color);
        }
    }
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 1, 0]), u32::MAX);
    assert_eq!(v.call("gdi32.dll", "DeleteObject", &[bitmap]), 0);
    assert_eq!(v.call("gdi32.dll", "SelectObject", &[dc, stock]), bitmap);
    assert_eq!(v.call("gdi32.dll", "DeleteObject", &[bitmap]), 1);
    assert_eq!(v.call("gdi32.dll", "GetObjectType", &[bitmap]), 0);
    assert_eq!(v.call("gdi32.dll", "DeleteDC", &[screen]), 0);
    assert_eq!(v.call("user32.dll", "ReleaseDC", &[0, dc]), 0);
    assert_eq!(v.call("gdi32.dll", "DeleteDC", &[dc]), 1);
    assert_eq!(v.call("user32.dll", "ReleaseDC", &[0, screen]), 1);
}

#[test]
fn dib_top_down_bottom_up_direct_writes_checked_permissions_and_release() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    for height in [-2, 2] {
        let (h, bits) = dib32(&mut v, dc, 2, height);
        let stock = v.call("gdi32.dll", "SelectObject", &[dc, h]);
        assert_eq!(
            v.mem.virtual_query(bits).unwrap().kind,
            wire::mem::MEM_MAPPED
        );
        let offset = if height < 0 { 0 } else { 8 };
        v.mem.write_u32(bits + offset, 0x7f123456).unwrap(); // CPU writes BGRA directly
        assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), 0x563412);
        assert_eq!(
            v.call("gdi32.dll", "SetPixel", &[dc, 1, 0, 0x332211]),
            0x332211
        );
        assert_eq!(v.mem.read_u32(bits + offset + 4).unwrap(), 0x112233);
        assert_eq!(
            v.call("gdi32.dll", "GetObjectW", &[h, 84, DATA + 0xe00]),
            84
        );
        assert_eq!(v.mem.read_u32(DATA + 0xe14).unwrap(), bits);
        assert_eq!(v.mem.read_u32(DATA + 0xe20).unwrap() as i32, height);
        assert_eq!(v.k("VirtualFree", &[bits, 0, 0x8000]), 0);
        assert_eq!(
            v.k(
                "VirtualProtect",
                &[bits, 1, wire::mem::PAGE_READONLY, DATA + 0xe80]
            ),
            1
        );
        assert_eq!(v.call("gdi32.dll", "SetPixel", &[dc, 0, 0, 0xff]), u32::MAX);
        assert_eq!(v.mem.read_u32(bits + offset).unwrap(), 0x7f123456);
        assert_eq!(v.call("gdi32.dll", "SelectObject", &[dc, stock]), h);
        assert_eq!(v.call("gdi32.dll", "DeleteObject", &[h]), 1);
        assert!(v.mem.read_u32(bits).is_err());
    }
}

#[test]
fn bitblt_overlap_clipping_origin_and_invalid_rop_preserve_expected_pixels() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    let (h, bits) = dib32(&mut v, dc, 4, -2);
    v.call("gdi32.dll", "SelectObject", &[dc, h]);
    for i in 0..8 {
        v.mem
            .write_u32(bits + i * 4, ((i + 1) << 16) | 0xff000000)
            .unwrap();
    }
    assert_eq!(
        v.call("gdi32.dll", "BitBlt", &[dc, 1, 0, 3, 1, dc, 0, 0, 0xcc0020]),
        1
    );
    let (_, _, pixels) = v.state.graphics.snapshot(&v.mem, dc).unwrap();
    assert_eq!(pixels, [1, 1, 2, 3, 5, 6, 7, 8]);
    assert_eq!(v.call("gdi32.dll", "SetWindowOrgEx", &[dc, 1, 0, 0]), 1);
    assert_eq!(
        v.call("gdi32.dll", "BitBlt", &[dc, 0, 1, 3, 1, dc, 1, 0, 0xcc0020]),
        1
    );
    let (_, _, pixels) = v.state.graphics.snapshot(&v.mem, dc).unwrap();
    assert_eq!(pixels, [1, 1, 2, 3, 1, 2, 7, 8]);
    assert_eq!(
        v.call("gdi32.dll", "BitBlt", &[dc, 1, 0, 1, 1, dc, 1, 1, 0xdead]),
        0
    );
    assert_eq!(v.state.graphics.snapshot(&v.mem, dc).unwrap().2, pixels);
}

#[cfg(target_os = "macos")]
#[test]
fn gdi_font_metrics_measure_the_selected_font_and_check_output() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    assert_eq!(v.call("gdi32.dll", "SetMapMode", &[dc, 1]), 1);
    assert_eq!(v.call("gdi32.dll", "GetMapMode", &[dc]), 1);
    assert_eq!(v.call("gdi32.dll", "SetMapMode", &[dc, 2]), 0);
    let mut heights = Vec::new();
    for size in [12i32, 36] {
        let mut lf = [0; 92];
        lf[..4].copy_from_slice(&(-size).to_le_bytes());
        lf[16..20].copy_from_slice(&400u32.to_le_bytes());
        for (i, c) in "Helvetica".encode_utf16().enumerate() {
            lf[28 + i * 2..30 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        v.mem.write_bytes(DATA, &lf).unwrap();
        let font = v.call("gdi32.dll", "CreateFontIndirectW", &[DATA]);
        assert_ne!(font, 0);
        let old = v.call("gdi32.dll", "SelectObject", &[dc, font]);
        assert_ne!(old, 0);
        v.mem.write_bytes(DATA + 0x100, &[0x55; 64]).unwrap();
        assert_eq!(
            v.call("gdi32.dll", "GetTextMetricsW", &[dc, DATA + 0x100]),
            1
        );
        let height = v.mem.read_u32(DATA + 0x100).unwrap();
        let ascent = v.mem.read_u32(DATA + 0x104).unwrap();
        let descent = v.mem.read_u32(DATA + 0x108).unwrap();
        assert_eq!(height, ascent + descent);
        assert!(height > 0 && ascent > 0);
        assert!(v.mem.read_u32(DATA + 0x114).unwrap() > 0);
        assert_eq!(v.mem.read_u32(DATA + 0x13c).unwrap(), 0x55555555);
        heights.push(height);
        assert_eq!(v.call("gdi32.dll", "GetTextMetricsW", &[dc, 0]), 0);
        assert_eq!(v.call("gdi32.dll", "SelectObject", &[dc, old]), font);
        assert_eq!(v.call("gdi32.dll", "DeleteObject", &[font]), 1);
    }
    assert!(heights[1] > heights[0] * 2);
    assert_eq!(v.call("gdi32.dll", "GetTextMetricsW", &[0, DATA]), 0);
    v.call("gdi32.dll", "DeleteDC", &[dc]);
}

#[test]
fn gdi_bitfields_dib_reports_masks_and_keeps_checked_pixels() {
    let mut v = Vm::new();
    let dc = v.call("gdi32.dll", "CreateCompatibleDC", &[0]);
    let info = [
        40u32,
        3,
        (-2i32) as u32,
        0x200001,
        3,
        24,
        0,
        0,
        0,
        0,
        0xff0000,
        0xff00,
        0xff,
    ];
    v.mem
        .write_bytes(
            DATA,
            &info
                .iter()
                .flat_map(|n| n.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let bitmap = v.call(
        "gdi32.dll",
        "CreateDIBSection",
        &[dc, DATA, 0, DATA + 0x100, 0, 0],
    );
    assert_ne!(bitmap, 0);
    let bits = v.mem.read_u32(DATA + 0x100).unwrap();
    v.mem.write_u32(bits, 0x11223344).unwrap();
    let old = v.call("gdi32.dll", "SelectObject", &[dc, bitmap]);
    assert_ne!(old, 0);
    assert_eq!(v.call("gdi32.dll", "GetPixel", &[dc, 0, 0]), 0x443322);
    assert_eq!(
        v.call("gdi32.dll", "GetObjectW", &[bitmap, 84, DATA + 0x200]),
        84
    );
    assert_eq!(v.mem.read_u32(DATA + 0x228).unwrap(), 3);
    assert_eq!(v.mem.read_u32(DATA + 0x240).unwrap(), 0xff0000);
    assert_eq!(v.mem.read_u32(DATA + 0x248).unwrap(), 0xff);
    v.mem.write_u32(DATA + 40, 0xff).unwrap();
    assert_eq!(
        v.call(
            "gdi32.dll",
            "CreateDIBSection",
            &[dc, DATA, 0, DATA + 0x100, 0, 0]
        ),
        0
    );
    v.call("gdi32.dll", "SelectObject", &[dc, old]);
    v.call("gdi32.dll", "DeleteObject", &[bitmap]);
    v.call("gdi32.dll", "DeleteDC", &[dc]);
}
