//! Logical font handles and x86 LOGFONT A/W marshaling. Font realization and
//! glyph rendering use a platform backend; a handle alone is no glyph result.
use super::{Arguments, Graphics};
use crate::mem::GuestMem;
use std::collections::BTreeMap;
pub(super) const SYSTEM_FONT: u32 = 0x42000080 + 13 * 4;
pub(super) struct Font {
    pub logfont: [u8; 92],
    stock: bool,
    metrics: Option<[u8; 60]>,
}
pub(super) fn stock_fonts() -> BTreeMap<u32, Font> {
    [10u32, 11, 12, 13, 14, 16, 17]
        .into_iter()
        .map(|id| {
            let mut lf = [0; 92];
            lf[..4].copy_from_slice(&(-16i32).to_le_bytes());
            lf[16..20].copy_from_slice(&400u32.to_le_bytes());
            lf[23] = 128;
            let face = if matches!(id, 10 | 11 | 16) {
                "MS Gothic"
            } else {
                "MS UI Gothic"
            };
            for (i, c) in face.encode_utf16().enumerate() {
                lf[28 + i * 2..30 + i * 2].copy_from_slice(&c.to_le_bytes());
            }
            (
                0x42000080 + id * 4,
                Font {
                    logfont: lf,
                    stock: true,
                    metrics: None,
                },
            )
        })
        .collect()
}
fn read_font(mem: &GuestMem, p: u32, wide: bool) -> Result<[u8; 92], u32> {
    let mut lf = [0; 92];
    if wide {
        mem.read_bytes(p, &mut lf).map_err(|_| 87u32)?;
    } else {
        let mut ansi = [0; 60];
        mem.read_bytes(p, &mut ansi).map_err(|_| 87u32)?;
        lf[..28].copy_from_slice(&ansi[..28]);
        let end = ansi[28..].iter().position(|&c| c == 0).unwrap_or(32);
        let text = crate::codepage::decode(&ansi[28..28 + end], 932).map_err(|_| 87u32)?;
        for (i, c) in text.encode_utf16().take(31).enumerate() {
            lf[28 + i * 2..30 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
    }
    lf[90..].fill(0);
    Ok(lf)
}
pub(super) fn dispatch(
    name: &str,
    a: &Arguments,
    mem: &GuestMem,
    gfx: &mut Graphics,
) -> Option<Result<u32, u32>> {
    Some((|| match name {
        "CreateFontA" | "CreateFontW" | "CreateFontIndirectA" | "CreateFontIndirectW" => {
            let wide = name.ends_with('W');
            let lf = if name.starts_with("CreateFontIndirect") {
                read_font(mem, a[0], wide)?
            } else {
                let mut lf = [0; 92];
                for (i, n) in a[..5].iter().enumerate() {
                    lf[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
                }
                for i in 0..8 {
                    lf[20 + i] = a[5 + i] as u8;
                }
                if a[13] != 0 {
                    let text = crate::menu::read_text(mem, a[13], wide)?.unwrap_or_default();
                    for (i, c) in text.into_iter().take(31).enumerate() {
                        lf[28 + i * 2..30 + i * 2].copy_from_slice(&c.to_le_bytes());
                    }
                }
                lf
            };
            let h = gfx.handle()?;
            gfx.fonts.insert(
                h,
                Font {
                    logfont: lf,
                    stock: false,
                    metrics: None,
                },
            );
            Ok(h)
        }
        "GetStockObject" if matches!(a[0], 10..=14 | 16 | 17) => Ok(0x42000080 + a[0] * 4),
        "GetTextMetricsW" => {
            let h = gfx.dc(a[0])?.font;
            let font = gfx.fonts.get_mut(&h).ok_or(6u32)?;
            if font.metrics.is_none() {
                font.metrics = Some(metrics(&font.logfont)?);
            }
            mem.write_bytes(a[1], font.metrics.as_ref().unwrap())
                .map_err(|_| 87u32)?;
            Ok(1)
        }
        "SelectObject" if gfx.fonts.contains_key(&a[1]) => {
            let dc = gfx.dcs.get_mut(&a[0]).ok_or(6u32)?;
            let old = dc.font;
            dc.font = a[1];
            Ok(old)
        }
        "DeleteObject" if gfx.fonts.contains_key(&a[0]) => {
            if !gfx.fonts[&a[0]].stock {
                if gfx.dcs.values().any(|d| d.font == a[0]) {
                    return Err(170);
                }
                gfx.fonts.remove(&a[0]);
            }
            Ok(1)
        }
        "GetObjectType" if gfx.fonts.contains_key(&a[0]) => Ok(6),
        "GetCurrentObject" if a[1] == 6 => Ok(gfx.dc(a[0])?.font),
        "GetObjectA" | "GetObjectW" if gfx.fonts.contains_key(&a[0]) => {
            let wide = name.ends_with('W');
            let size = if wide { 92 } else { 60 };
            if a[2] == 0 {
                return Ok(size);
            }
            if a[1] < size {
                return Err(87);
            }
            let lf = &gfx.fonts[&a[0]].logfont;
            if wide {
                mem.write_bytes(a[2], lf).map_err(|_| 87u32)?;
            } else {
                let mut out = [0; 60];
                out[..28].copy_from_slice(&lf[..28]);
                let units: Vec<_> = lf[28..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes(*b))
                    .take_while(|&n| n != 0)
                    .collect();
                let ansi = crate::codepage::encode(&String::from_utf16_lossy(&units), 932)
                    .map_err(|_| 87u32)?
                    .0;
                let count = ansi.len().min(31);
                out[28..28 + count].copy_from_slice(&ansi[..count]);
                mem.write_bytes(a[2], &out).map_err(|_| 87u32)?;
            }
            Ok(size)
        }
        _ => Err(u32::MAX),
    })())
    .filter(|r| !matches!(r, Err(u32::MAX)))
}

fn metrics(lf: &[u8; 92]) -> Result<[u8; 60], u32> {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn wire_font_metrics(
                face: *const u16,
                length: u32,
                height: i32,
                width: i32,
                weight: i32,
                italic: i32,
                values: *mut i32,
                characters: *mut u16,
                pitch: *mut u8,
            ) -> i32;
        }
        let face: Vec<_> = lf[28..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .take_while(|&c| c != 0)
            .collect();
        let mut values = [0; 11];
        let mut characters = [0; 4];
        let mut pitch = 0;
        let int = |i| i32::from_le_bytes(lf[i..i + 4].try_into().unwrap());
        if unsafe {
            wire_font_metrics(
                face.as_ptr(),
                face.len() as u32,
                int(0),
                int(4),
                int(16),
                lf[20] as i32,
                values.as_mut_ptr(),
                characters.as_mut_ptr(),
                &mut pitch,
            )
        } == 0
        {
            return Err(120);
        }
        let mut out = [0; 60];
        for (i, n) in values.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&n.to_le_bytes());
        }
        for (i, n) in characters.iter().enumerate() {
            out[44 + i * 2..46 + i * 2].copy_from_slice(&n.to_le_bytes());
        }
        out[52..57].copy_from_slice(&[lf[20], lf[21], lf[22], pitch, lf[23]]);
        Ok(out)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = lf;
        Err(120)
    }
}
