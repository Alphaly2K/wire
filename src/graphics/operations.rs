//! Checked PE32 object/raster operations, independent of process and API state.
use super::GdiOperation as Op;
use super::{
    dib, font,
    surface::{Bitmap, Storage},
    Arguments, Graphics, STOCK_BITMAP,
};
use crate::mem::GuestMem;

fn point_output(mem: &GuestMem, p: u32, xy: [i32; 2]) -> Result<(), u32> {
    if p != 0 {
        mem.write_bytes(
            p,
            &xy.into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .map_err(|_| 87u32)?;
    }
    Ok(())
}
fn bitmap_object(mem: &GuestMem, bitmap: &Bitmap, a: &Arguments) -> Result<u32, u32> {
    let dib = bitmap.dib_base();
    let size = if dib.is_some() { 84 } else { 24 };
    if a[2] == 0 {
        return Ok(size);
    }
    if a[1] < 24 {
        return Err(87);
    }
    let mut bytes = Vec::new();
    for v in [0, bitmap.width, bitmap.height, bitmap.stride()] {
        bytes.extend(v.to_le_bytes());
    }
    bytes.extend(1u16.to_le_bytes());
    bytes.extend((bitmap.bpp as u16).to_le_bytes());
    bytes.extend(dib.unwrap_or(0).to_le_bytes());
    if dib.is_some() && a[1] >= 84 {
        let height = if matches!(bitmap.storage, Storage::Dib { top_down: true, .. }) {
            -(bitmap.height as i32)
        } else {
            bitmap.height as i32
        };
        for v in [
            40,
            bitmap.width,
            height as u32,
            0x00200001,
            bitmap.compression,
            bitmap.size(),
            0,
            0,
            0,
            0,
            if bitmap.compression == 3 { 0xff0000 } else { 0 },
            if bitmap.compression == 3 { 0xff00 } else { 0 },
            if bitmap.compression == 3 { 0xff } else { 0 },
            0,
            0,
        ] {
            bytes.extend(v.to_le_bytes());
        }
        debug_assert_eq!(bytes.len(), 84);
    }
    mem.write_bytes(a[2], &bytes).map_err(|_| 87u32)?;
    Ok(bytes.len() as u32)
}
fn blit(a: &Arguments, mem: &GuestMem, gfx: &mut Graphics) -> Result<u32, u32> {
    if a[8] != 0x00cc0020 {
        return Err(120);
    } // SRCCOPY only
    let dst = gfx.dc(a[0])?;
    let src = gfx.dc(a[5])?;
    let target = dst.bitmap;
    let source = src.bitmap;
    let [dx, dy] = dst.point(a[1], a[2]);
    let [sx, sy] = src.point(a[6], a[7]);
    let w = a[3] as i32 as i64;
    let h = a[4] as i32 as i64;
    if w < 0 || h < 0 {
        return Err(87);
    }
    let d = gfx.bitmaps.get(&target).ok_or(6u32)?;
    let clip = dst.bounds(d.width, d.height);
    let x0 = 0.max(clip[0] - dx);
    let y0 = 0.max(clip[1] - dy);
    let x1 = w.min(clip[2] - dx);
    let y1 = h.min(clip[3] - dy);
    if x1 <= x0 || y1 <= y0 {
        return Ok(1);
    }
    let s = gfx.bitmaps.get(&source).ok_or(6u32)?;
    if sx + x0 < 0 || sy + y0 < 0 || sx + x1 > s.width as i64 || sy + y1 > s.height as i64 {
        return Err(87);
    }
    // Validate both surfaces before writes, then snapshot for same-bitmap
    // overlap. No host pointer escapes, and direct guest writes are visible.
    s.check(mem, false)?;
    d.check(mem, true)?;
    let mut pixels = Vec::with_capacity(((x1 - x0) * (y1 - y0)) as usize);
    for y in y0..y1 {
        for x in x0..x1 {
            pixels.push(s.read(mem, (sx + x) as u32, (sy + y) as u32)?);
        }
    }
    let d = gfx.bitmaps.get_mut(&target).unwrap();
    let mut values = pixels.into_iter();
    for y in y0..y1 {
        for x in x0..x1 {
            d.write(
                mem,
                (dx + x) as u32,
                (dy + y) as u32,
                values.next().unwrap(),
            )?;
        }
    }
    Ok(1)
}

pub(crate) fn execute(
    op: Op,
    a: &Arguments,
    mem: &mut GuestMem,
    gfx: &mut Graphics,
) -> Result<u32, u32> {
    if let Some(result) = font::dispatch(op, a, mem, gfx) {
        return result;
    }
    if let Some(result) = dib::dispatch(op, a, mem, gfx) {
        return result;
    }
    match op {
        Op::SetMapMode | Op::GetMapMode => {
            gfx.dc(a[0])?;
            if op == Op::SetMapMode && a[1] != 1 {
                return Err(120);
            }
            Ok(1) // only MM_TEXT is currently supported
        }
        Op::CompatibleBitmap => {
            let bpp = gfx.bitmaps.get(&gfx.dc(a[0])?.bitmap).ok_or(6u32)?.bpp;
            let (width, height, bpp) = if a[1] == 0 || a[2] == 0 {
                (1, 1, 1)
            } else {
                if (a[1] as i32) < 0 || (a[2] as i32) < 0 {
                    return Err(87);
                }
                (a[1], a[2], bpp)
            };
            gfx.capacity(width, height)?;
            let h = gfx.handle()?;
            gfx.bitmaps
                .insert(h, Bitmap::owned(width, height, bpp, false)?);
            Ok(h)
        }
        Op::DibSection => {
            if a[0] != 0 {
                gfx.dc(a[0])?;
            }
            if a[2] != 0 || a[4] != 0 || a[5] != 0 {
                return Err(120);
            }
            if a[3] == 0 {
                return Err(87);
            }
            let mut info = [0; 40];
            mem.read_bytes(a[1], &mut info).map_err(|_| 87u32)?;
            let f: [u32; 10] = std::array::from_fn(|i| {
                u32::from_le_bytes(info[i * 4..i * 4 + 4].try_into().unwrap())
            });
            if f[0] != 40 || f[3] != 0x00200001 || !matches!(f[4], 0 | 3) || f[8] != 0 {
                return Err(120);
            }
            if f[4] == 3 {
                let mut masks = [0; 12];
                mem.read_bytes(a[1].checked_add(40).ok_or(87u32)?, &mut masks)
                    .map_err(|_| 87u32)?;
                if masks
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .copied()
                    .map(u32::from_le_bytes)
                    .collect::<Vec<_>>()
                    != [0xff0000, 0xff00, 0xff]
                {
                    return Err(120);
                }
            }
            let width = f[1] as i32;
            let height = f[2] as i32;
            if width <= 0 || height == 0 || height == i32::MIN {
                return Err(87);
            }
            let height_abs = height.unsigned_abs();
            gfx.capacity(width as u32, height_abs)?;
            mem.ptr(a[3], 4, true).map_err(|_| 87u32)?;
            let h = gfx.handle()?;
            let base = mem.alloc_dib(width as u32 * height_abs * 4)?;
            if mem.write_u32(a[3], base).is_err() {
                mem.free_dib(base)?;
                return Err(87);
            }
            gfx.bitmaps.insert(
                h,
                Bitmap {
                    width: width as u32,
                    height: height_abs,
                    bpp: 32,
                    compression: f[4],
                    storage: Storage::Dib {
                        base,
                        top_down: height < 0,
                    },
                    internal: false,
                },
            );
            Ok(h)
        }
        Op::Select => {
            if gfx.dc(a[0])?.window.is_some() {
                return Err(87);
            }
            let bitmap = gfx.bitmaps.get(&a[1]).ok_or(6u32)?;
            if bitmap.internal && a[1] != STOCK_BITMAP {
                return Err(87);
            }
            if a[1] != STOCK_BITMAP
                && gfx
                    .dcs
                    .iter()
                    .any(|(&h, dc)| h != a[0] && dc.bitmap == a[1])
            {
                return Err(170);
            }
            let dc = gfx.dcs.get_mut(&a[0]).unwrap();
            let old = dc.bitmap;
            dc.bitmap = a[1];
            Ok(old)
        }
        Op::Delete => {
            let b = gfx.bitmaps.get(&a[0]).ok_or(6u32)?;
            if b.internal || gfx.dcs.values().any(|dc| dc.bitmap == a[0]) {
                return Err(170);
            }
            if let Some(base) = b.dib_base() {
                mem.free_dib(base)?;
            }
            gfx.bitmaps.remove(&a[0]);
            Ok(1)
        }
        Op::ObjectA | Op::ObjectW => bitmap_object(mem, gfx.bitmaps.get(&a[0]).ok_or(6u32)?, a),
        Op::ObjectType => Ok(if gfx.bitmaps.contains_key(&a[0]) {
            7
        } else if let Some(dc) = gfx.dcs.get(&a[0]) {
            if dc.window.is_none() {
                10
            } else {
                3
            }
        } else {
            0
        }),
        Op::CurrentObject => {
            if a[1] != 7 {
                return Err(120);
            }
            Ok(gfx.dc(a[0])?.bitmap)
        }
        Op::StockObject => {
            if a[0] != 21 {
                return Err(120);
            }
            Ok(STOCK_BITMAP)
        }
        Op::GetPixel | Op::SetPixel | Op::SetPixelV => {
            let dc = gfx.dc(a[0])?;
            let bitmap = dc.bitmap;
            let [x, y] = dc.point(a[1], a[2]);
            let b = gfx.bitmaps.get(&bitmap).ok_or(6u32)?;
            let clip = dc.bounds(b.width, b.height);
            if x < clip[0] || x >= clip[2] || y < clip[1] || y >= clip[3] {
                return Err(87);
            }
            if x < 0 || y < 0 || x > u32::MAX as i64 || y > u32::MAX as i64 {
                return Err(87);
            }
            if op == Op::GetPixel {
                gfx.bitmaps
                    .get(&bitmap)
                    .ok_or(6u32)?
                    .read(mem, x as u32, y as u32)
            } else if a[3] & 0xff000000 != 0 {
                Err(120)
            } else {
                let color = gfx
                    .bitmaps
                    .get_mut(&bitmap)
                    .ok_or(6u32)?
                    .write(mem, x as u32, y as u32, a[3])?;
                Ok(if op == Op::SetPixelV { 1 } else { color })
            }
        }
        Op::Blit => blit(a, mem, gfx),
        Op::SetWindowOrigin
        | Op::SetViewportOrigin
        | Op::GetWindowOrigin
        | Op::GetViewportOrigin => {
            gfx.dc(a[0])?;
            let dc = gfx.dcs.get_mut(&a[0]).unwrap();
            let slot = if matches!(op, Op::SetWindowOrigin | Op::GetWindowOrigin) {
                &mut dc.origin
            } else {
                &mut dc.viewport
            };
            if matches!(op, Op::SetWindowOrigin | Op::SetViewportOrigin) {
                point_output(mem, a[3], *slot)?;
                *slot = [a[1] as i32, a[2] as i32];
            } else {
                if a[1] == 0 {
                    return Err(87);
                }
                point_output(mem, a[1], *slot)?;
            }
            Ok(1)
        }
        Op::SetTextColor | Op::GetTextColor | Op::SetBackgroundColor | Op::GetBackgroundColor => {
            gfx.dc(a[0])?;
            let dc = gfx.dcs.get_mut(&a[0]).unwrap();
            let slot = if matches!(op, Op::SetTextColor | Op::GetTextColor) {
                &mut dc.text_color
            } else {
                &mut dc.bk_color
            };
            let old = *slot;
            if matches!(op, Op::SetTextColor | Op::SetBackgroundColor) {
                if a[1] & 0xff000000 != 0 {
                    return Err(120);
                }
                *slot = a[1];
            }
            Ok(old)
        }
        Op::Flush => Ok(1), // all implemented operations are synchronous
        _ => unreachable!(),
    }
}
