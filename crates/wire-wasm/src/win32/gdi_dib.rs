use super::{bitmap, layout, pixels, Bitmap, Failure, Gdi, Storage, O};
use crate::{
    machine::Mode,
    memory::{Memory, WRITE},
};

fn words(memory: &Memory, pointer: u64) -> Result<[u32; 10], Failure> {
    let mut bytes = [0; 40];
    memory.read(pointer, &mut bytes)?;
    Ok(std::array::from_fn(|i| {
        u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
    }))
}
fn format_error(code: u32) -> Failure {
    Failure::Windows(if code == 120 { 50 } else { code })
}
fn read_layout(memory: &Memory, pointer: u64) -> Result<layout::Layout, Failure> {
    let l = layout::Layout::parse(words(memory, pointer)?).map_err(format_error)?;
    if l.compression == 3 {
        let mut masks = [0; 12];
        memory.read(
            pointer.checked_add(40).ok_or(Failure::Windows(87))?,
            &mut masks,
        )?;
        layout::Layout::masks(std::array::from_fn(|i| {
            u32::from_le_bytes(masks[i * 4..i * 4 + 4].try_into().unwrap())
        }))
        .map_err(format_error)?;
    }
    pixels::dimensions(l.width, l.height)?;
    Ok(l)
}
fn header(l: layout::Layout) -> [u32; 10] {
    [
        40,
        l.width,
        if l.top_down {
            (-(l.height as i32)) as u32
        } else {
            l.height
        },
        0x00200001,
        l.compression,
        l.width * l.height * 4,
        0,
        0,
        0,
        0,
    ]
}

pub(super) fn object(b: &Bitmap, a: &[u64; 16], memory: &mut Memory) -> Result<u64, Failure> {
    let x86 = memory.mode() == Mode::X86;
    let bm = if x86 { 24 } else { 32 };
    let full = if b.dib().is_some() {
        if x86 {
            84
        } else {
            104
        }
    } else {
        bm
    };
    if a[2] == 0 {
        return Ok(full as u64);
    }
    let capacity = a[1] as u32 as i32;
    if capacity < bm as i32 {
        return Err(Failure::Windows(87));
    }
    let size = if capacity >= full as i32 { full } else { bm };
    let mut output = bitmap::bytes(size)?;
    for (n, v) in [
        0,
        b.width,
        b.height,
        if b.dib().is_some() {
            b.width * 4
        } else {
            (b.width * b.bpp).div_ceil(16) * 2
        },
    ]
    .into_iter()
    .enumerate()
    {
        output[n * 4..n * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    output[16..18].copy_from_slice(&1u16.to_le_bytes());
    output[18..20].copy_from_slice(&(b.bpp as u16).to_le_bytes());
    if let Some((base, l)) = b.dib() {
        let at = if x86 { 20 } else { 24 };
        let ptr = if x86 { 4 } else { 8 };
        output[at..at + ptr].copy_from_slice(&base.to_le_bytes()[..ptr]);
        if size == full {
            for (n, v) in header(l).into_iter().enumerate() {
                output[bm + n * 4..bm + n * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            if l.compression == 3 {
                for (n, v) in [0xff0000u32, 0xff00, 0xff].into_iter().enumerate() {
                    output[bm + 40 + n * 4..bm + 44 + n * 4].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
    memory.write(a[2], &output)?;
    Ok(size as u64)
}

pub(super) fn dispatch(
    id: O,
    a: &[u64; 16],
    memory: &mut Memory,
    g: &mut Gdi,
) -> Result<u64, Failure> {
    match id {
        O::DibSection => {
            if a[0] != 0 {
                g.dcs.validate(a[0])?;
            }
            if a[2] != 0 || a[4] != 0 || a[5] != 0 {
                return Err(Failure::Windows(50));
            }
            if a[3] == 0 {
                return Err(Failure::Windows(87));
            }
            let l = read_layout(memory, a[1])?;
            let ptr = memory.mode() as usize / 8;
            memory.check(a[3], ptr, WRITE)?;
            g.capacity(l.width, l.height, None)?;
            let handle = g.handle(1)?;
            let size = u64::from(l.width) * u64::from(l.height) * 4;
            let base = memory.map_dib(size)?;
            if let Err(f) = memory.write_int(a[3], ptr, base) {
                memory
                    .unmap(base, size.div_ceil(4096) * 4096)
                    .expect("unpublished DIB allocation");
                return Err(Failure::Fault(f));
            }
            g.bitmaps.insert(
                handle,
                Bitmap {
                    width: l.width,
                    height: l.height,
                    bpp: 32,
                    storage: Storage::Dib { base, layout: l },
                    internal: false,
                },
            );
            g.next += 16;
            Ok(handle)
        }
        O::GetDib => {
            g.dcs.validate(a[0])?;
            let b = g.bitmap(a[1])?;
            if a[6] != 0 {
                return Err(Failure::Windows(50));
            }
            let mut info = words(memory, a[5])?;
            if info[0] != 40 {
                return Err(Failure::Windows(50));
            }
            if a[4] == 0 {
                info[1] = b.width;
                info[2] = b.height;
                info[3] = 0x00200001;
                info[4] = 0;
                info[5] = b.size() as u32;
                let mut output = [0; 40];
                for (n, v) in info.into_iter().enumerate() {
                    output[n * 4..n * 4 + 4].copy_from_slice(&v.to_le_bytes());
                }
                memory.write(a[5], &output)?;
                return Ok(u64::from(b.height));
            }
            if info[1] != b.width
                || (info[2] as i32).unsigned_abs() != b.height
                || info[3] != 0x00200001
                || info[4] != 0
                || info[8] != 0
            {
                return Err(Failure::Windows(50));
            }
            if g.selected_elsewhere(a[1], 0) {
                return Err(Failure::Windows(87));
            }
            let start = a[2] as u32;
            let count = (a[3] as u32).min(b.height.saturating_sub(start));
            if count == 0 {
                return Ok(0);
            }
            let length = count as usize * b.width as usize * 4;
            memory.check(a[4], length, WRITE)?;
            let top_down = (info[2] as i32) < 0;
            let y = if top_down {
                start
            } else {
                b.height - start - count
            };
            let values = b.read_rect(memory, [0, y, b.width, y + count])?;
            let mut output = bitmap::bytes(length)?;
            for row in 0..count {
                let source =
                    if top_down { row } else { count - row - 1 } as usize * b.width as usize;
                let target = row as usize * b.width as usize * 4;
                for (p, &c) in output[target..target + b.width as usize * 4]
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(&values[source..source + b.width as usize])
                {
                    p.copy_from_slice(&layout::swap_rb(c).to_le_bytes());
                }
            }
            memory.write(a[4], &output)?;
            Ok(u64::from(count))
        }
        O::StretchDib => {
            if a[11] != 0 || a[12] != 0xcc0020 {
                return Err(Failure::Windows(50));
            }
            let l = read_layout(memory, a[10])?;
            let signed = |n: usize| i64::from(a[n] as u32 as i32);
            let (dw, dh, sw, sh) = (signed(3), signed(4), signed(7), signed(8));
            if [dw, dh, sw, sh].into_iter().any(|v| v <= 0) {
                return Err(Failure::Windows(50));
            }
            let (sx, sy) = (signed(5), signed(6));
            if sx < 0 || sy < 0 || sx + sw > i64::from(l.width) || sy + sh > i64::from(l.height) {
                return Err(Failure::Windows(87));
            }
            let d = g.dcs.state(a[0])?;
            let target = d.bitmap;
            let dx = signed(1) - i64::from(d.origin[0]) + i64::from(d.viewport[0]);
            let dy = signed(2) - i64::from(d.origin[1]) + i64::from(d.viewport[1]);
            let dst = g.bitmap(target)?;
            let clip = d
                .clip
                .unwrap_or([0, 0, dst.width as i32, dst.height as i32]);
            let x0 = 0.max(i64::from(clip[0].max(0)) - dx);
            let y0 = 0.max(i64::from(clip[1].max(0)) - dy);
            let x1 = dw.min(i64::from(clip[2].min(dst.width as i32)) - dx);
            let y1 = dh.min(i64::from(clip[3].min(dst.height as i32)) - dy);
            if x1 <= x0 || y1 <= y0 {
                return Ok(sh as u64);
            }
            let dr = [
                (dx + x0) as u32,
                (dy + y0) as u32,
                (dx + x1) as u32,
                (dy + y1) as u32,
            ];
            dst.check_rect(memory, dr, true)?;
            let mut source = bitmap::bytes(l.width as usize * l.height as usize * 4)?;
            memory.read(a[9], &mut source)?;
            let mut values = pixels::allocate((x1 - x0) as u32, (y1 - y0) as u32)?;
            for (y, row) in (y0..y1).zip(values.chunks_exact_mut((x1 - x0) as usize)) {
                for (x, value) in (x0..x1).zip(row) {
                    let ix = sx + x * sw / dw;
                    let sample = y * sh / dh;
                    let iy = if l.top_down {
                        sy + sample
                    } else {
                        i64::from(l.height) - sy - sh + sample
                    };
                    let at = l.offset(ix as u32, iy as u32) as usize;
                    *value =
                        layout::swap_rb(u32::from_le_bytes(source[at..at + 4].try_into().unwrap()))
                            & 0xffffff;
                }
            }
            g.bitmaps
                .get_mut(&target)
                .unwrap()
                .write_rect(memory, dr, &values)?;
            Ok(sh as u64)
        }
        _ => unreachable!(),
    }
}
