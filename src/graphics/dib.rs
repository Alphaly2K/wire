//! Checked 32bpp BI_RGB transfer contracts for the initial raster backend.
use super::{surface::dimensions, Arguments, Graphics};
use crate::mem::GuestMem;
fn words(mem: &GuestMem, p: u32) -> Result<[u32; 10], u32> {
    let mut bytes = [0; 40];
    mem.read_bytes(p, &mut bytes).map_err(|_| 87u32)?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
pub(super) fn dispatch(
    name: &str,
    a: &Arguments,
    mem: &GuestMem,
    gfx: &mut Graphics,
) -> Option<Result<u32, u32>> {
    if !matches!(name, "StretchDIBits" | "GetDIBits") {
        return None;
    }
    Some((|| match name {
        "GetDIBits" => {
            gfx.dc(a[0])?;
            let bitmap = gfx.bitmaps.get(&a[1]).ok_or(6u32)?;
            if a[6] != 0 {
                return Err(120);
            }
            let mut info = words(mem, a[5])?;
            if info[0] != 40 {
                return Err(120);
            }
            if a[4] == 0 {
                info[1] = bitmap.width;
                info[2] = bitmap.height;
                info[3] = 1 | (32 << 16);
                info[4] = 0;
                info[5] = bitmap.size();
                mem.write_bytes(
                    a[5],
                    &info
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>(),
                )
                .map_err(|_| 87u32)?;
                return Ok(bitmap.height);
            }
            if info[1] != bitmap.width
                || (info[2] as i32).unsigned_abs() != bitmap.height
                || info[3] != 1 | (32 << 16)
                || info[4] != 0
            {
                return Err(120);
            }
            if gfx.dcs.values().any(|dc| dc.bitmap == a[1]) {
                return Err(87);
            }
            let count = a[3].min(bitmap.height.saturating_sub(a[2]));
            mem.ptr(
                a[4],
                count.checked_mul(bitmap.width * 4).ok_or(87u32)?,
                true,
            )
            .map_err(|_| 87u32)?;
            bitmap.check(mem, false)?;
            let mut bytes = Vec::with_capacity((count * bitmap.width * 4) as usize);
            for y in 0..count {
                let y = if (info[2] as i32) > 0 {
                    bitmap.height - 1 - a[2] - y
                } else {
                    a[2] + y
                };
                for x in 0..bitmap.width {
                    let c = bitmap.read(mem, x, y)?;
                    bytes.extend(
                        ((c & 0xff00) | ((c & 255) << 16) | ((c >> 16) & 255)).to_le_bytes(),
                    );
                }
            }
            mem.write_bytes(a[4], &bytes).map_err(|_| 87u32)?;
            Ok(count)
        }
        "StretchDIBits" => {
            if a[11] != 0 || a[12] != 0xcc0020 {
                return Err(120);
            }
            let info = words(mem, a[10])?;
            if info[0] != 40
                || info[3] != 1 | (32 << 16)
                || info[4] != 0
                || info[1] as i32 <= 0
                || info[2] == 0
                || info[2] == 0x80000000
            {
                return Err(120);
            }
            let width = info[1];
            let height = (info[2] as i32).unsigned_abs();
            let count = dimensions(width, height)?;
            if [a[3], a[4], a[7], a[8]].iter().any(|&n| n as i32 <= 0) {
                return Err(120);
            }
            let sx = a[5] as i32 as i64;
            let sy = a[6] as i32 as i64;
            if sx < 0
                || sy < 0
                || sx + a[7] as i64 > width as i64
                || sy + a[8] as i64 > height as i64
            {
                return Err(87);
            }
            // Snapshot source before any target write, including overlapping guest DIB storage.
            let mut src = vec![0; count as usize * 4];
            mem.read_bytes(a[9], &mut src).map_err(|_| 87u32)?;
            let dc = gfx.dc(a[0])?;
            let target = dc.bitmap;
            let [dx, dy] = dc.point(a[1], a[2]);
            let b = &gfx.bitmaps[&target];
            let clip = dc.bounds(b.width, b.height);
            let dst = gfx.bitmaps.get_mut(&target).ok_or(6u32)?;
            dst.check(mem, true)?;
            let x0 = 0.max(clip[0] - dx);
            let y0 = 0.max(clip[1] - dy);
            let x1 = (a[3] as i64).min(clip[2] - dx);
            let y1 = (a[4] as i64).min(clip[3] - dy);
            for y in y0..y1 {
                for x in x0..x1 {
                    let ix = sx + x * a[7] as i64 / a[3] as i64;
                    let iy = sy + y * a[8] as i64 / a[4] as i64;
                    let iy = if info[2] as i32 > 0 {
                        height as i64 - 1 - iy
                    } else {
                        iy
                    };
                    let off = (iy * width as i64 + ix) as usize * 4;
                    let c = u32::from_le_bytes(src[off..off + 4].try_into().unwrap());
                    dst.write(
                        mem,
                        (dx + x) as u32,
                        (dy + y) as u32,
                        (c & 0xff00) | ((c & 255) << 16) | ((c >> 16) & 255),
                    )?;
                }
            }
            Ok(a[8])
        }
        _ => unreachable!(),
    })())
}
