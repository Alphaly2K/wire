//! Shared DC identities and bounded raster storage for both guest ABIs.
use super::{
    call::Call,
    catalog::{GdiApi as O, GdiDcApi as D},
    user, ServiceError,
};
use crate::{
    machine::{Mode, State},
    memory::Memory,
};
use std::collections::BTreeMap;
pub(crate) struct Resizes(Vec<(u64, Bitmap)>);
#[path = "gdi_bitmap.rs"]
mod bitmap;
#[path = "../../../../src/win32/user/colors.rs"]
mod colors;
#[path = "gdi_dib.rs"]
mod dib;
#[path = "gdi_font.rs"]
mod font;
#[path = "../../../../src/win32/gdi/dc.rs"]
mod identities;
#[path = "../../../../src/graphics/dib_layout.rs"]
mod layout;
#[path = "../../../../src/graphics/pixels.rs"]
mod pixels;
use bitmap::{Bitmap, Storage};

#[derive(Default)]
struct Dc {
    bitmap: u64,
    brush: u64,
    font: u64,
    origin: [i32; 2],
    viewport: [i32; 2],
    text: u32,
    background: u32,
    clip: Option<[i32; 4]>,
    paint: bool,
}
pub(crate) struct Gdi {
    dcs: identities::DeviceContexts<Dc>,
    bitmaps: BTreeMap<u64, Bitmap>,
    windows: BTreeMap<u64, u64>,
    next: u64,
    end: u64,
    stock: u64,
    brush: u64,
    fonts: BTreeMap<u64, font::values::Font>,
    font_base: u64,
    system_font: u64,
}

impl Gdi {
    pub fn new(mode: Mode) -> Self {
        let start = if mode == Mode::X86 {
            0x42000000
        } else {
            0x7ffd01000000
        };
        Self {
            dcs: identities::DeviceContexts::new(start + 0x10000, start + 0x1000000),
            bitmaps: BTreeMap::from([(
                start + 4,
                Bitmap {
                    width: 1,
                    height: 1,
                    bpp: 1,
                    storage: Storage::Owned(vec![0]),
                    internal: true,
                },
            )]),
            windows: BTreeMap::new(),
            next: start + 0x1010000,
            end: start + 0x2000000,
            stock: start + 4,
            brush: start + 0x40,
            fonts: if mode == Mode::X86 {
                font::values::STOCK_IDS
                    .into_iter()
                    .map(|id| {
                        (
                            start + 0x80 + u64::from(id) * 4,
                            font::values::Font::stock(id),
                        )
                    })
                    .collect()
            } else {
                BTreeMap::new()
            },
            font_base: start + 0x80,
            system_font: if mode == Mode::X86 {
                start + 0x80 + u64::from(font::values::SYSTEM_ID) * 4
            } else {
                0
            },
        }
    }
    fn capacity(&self, width: u32, height: u32, replacing: Option<u64>) -> Result<(), u32> {
        let size = u64::from(pixels::dimensions(width, height)?) * 4;
        let used = self
            .bitmaps
            .iter()
            .filter(|(h, _)| Some(**h) != replacing)
            .map(|(_, b)| b.size())
            .sum::<u64>();
        if used + size > 256 * 1024 * 1024 {
            Err(8)
        } else {
            Ok(())
        }
    }
    fn handle(&self, count: usize) -> Result<u64, u32> {
        if self.count() + count > 4096
            || self
                .next
                .checked_add((count as u64) * 16)
                .is_none_or(|n| n > self.end)
        {
            Err(8)
        } else {
            Ok(self.next)
        }
    }
    fn count(&self) -> usize {
        self.bitmaps.len() + self.dcs.count() + self.fonts.len()
    }
    pub fn resize(&mut self, window: u64, width: u32, height: u32) -> Result<(), u32> {
        let prepared = self.prepare_resizes(&[(window, width, height)])?;
        self.commit_resizes(prepared);
        Ok(())
    }
    pub(crate) fn prepare_resizes(&self, requests: &[(u64, u32, u32)]) -> Result<Resizes, u32> {
        let mut replacements = BTreeMap::new();
        for &(window, width, height) in requests {
            if let Some(&handle) = self.windows.get(&window) {
                let old = &self.bitmaps[&handle];
                let (width, height) = (width.max(1), height.max(1));
                if old.width != width || old.height != height {
                    replacements.insert(handle, (width, height));
                }
            }
        }
        if replacements.is_empty() {
            return Ok(Resizes(Vec::new()));
        }
        let mut bytes = self
            .bitmaps
            .iter()
            .filter(|(h, _)| !replacements.contains_key(h))
            .map(|(_, b)| b.size())
            .sum::<u64>();
        for &(width, height) in replacements.values() {
            bytes += u64::from(pixels::dimensions(width, height)?) * 4;
        }
        if bytes > 256 * 1024 * 1024 {
            return Err(8);
        }
        let mut prepared = Vec::new();
        for (handle, (width, height)) in replacements {
            let old = &self.bitmaps[&handle];
            let mut data = pixels::allocate(width, height)?;
            for y in 0..height.min(old.height) {
                let count = width.min(old.width) as usize;
                data[y as usize * width as usize..y as usize * width as usize + count]
                    .copy_from_slice(
                        &old.owned()[y as usize * old.width as usize
                            ..y as usize * old.width as usize + count],
                    );
            }
            prepared.push((
                handle,
                Bitmap {
                    width,
                    height,
                    bpp: 32,
                    storage: Storage::Owned(data),
                    internal: true,
                },
            ));
        }
        Ok(Resizes(prepared))
    }
    pub(crate) fn commit_resizes(&mut self, prepared: Resizes) {
        for (handle, bitmap) in prepared.0 {
            self.bitmaps.insert(handle, bitmap);
        }
    }
    pub(crate) fn check_dc(&self, handle: u64) -> Result<(), u32> {
        self.dcs.validate(handle)
    }
    pub fn acquire(
        &mut self,
        window: u64,
        tid: u32,
        width: u32,
        height: u32,
        paint: Option<[i32; 4]>,
    ) -> Result<u64, u32> {
        self.dcs.preview(window, false, |_| true)?;
        let existing = self.windows.get(&window).copied();
        if self.count() + usize::from(existing.is_none()) + 1 > 4096 {
            return Err(8);
        }
        if existing.is_none() {
            self.handle(1)?;
        }
        let bitmap = if let Some(bitmap) = existing {
            self.resize(window, width, height)?;
            bitmap
        } else {
            let width = width.max(1);
            let height = height.max(1);
            self.capacity(width, height, None)?;
            let data = pixels::allocate(width, height)?;
            let handle = self.next;
            self.next += 16;
            self.bitmaps.insert(
                handle,
                Bitmap {
                    width,
                    height,
                    bpp: 32,
                    storage: Storage::Owned(data),
                    internal: true,
                },
            );
            self.windows.insert(window, handle);
            handle
        };
        let handle = self.dcs.acquire(window, u64::from(tid), false, |_| true)?;
        *self.dcs.state_mut(handle)? = Dc {
            bitmap,
            background: 0xffffff,
            brush: self.brush,
            font: self.system_font,
            clip: paint,
            paint: paint.is_some(),
            ..Dc::default()
        };
        Ok(handle)
    }
    pub fn release(&mut self, window: u64, handle: u64, tid: u32, paint: bool) -> Result<u64, u32> {
        if self.dcs.state(handle)?.paint != paint {
            return Err(6);
        }
        self.dcs.release(handle, window, u64::from(tid), false)
    }
    pub fn cancel_paint(&mut self, handle: u64) {
        if let Ok((window, tid, false)) = self.dcs.information(handle) {
            if self.dcs.state(handle).is_ok_and(|d| d.paint) {
                let _ = self.dcs.release(handle, window, tid, false);
            }
        }
    }
    pub fn remove_window(&mut self, window: u64) {
        self.dcs.remove_window(window);
        if let Some(bitmap) = self.windows.remove(&window) {
            self.bitmaps.remove(&bitmap);
        }
    }
    pub fn exit_thread(&mut self, tid: u32) {
        self.dcs.exit_thread(u64::from(tid));
    }
    pub fn check_clear(&self, memory: &Memory) -> Result<(), String> {
        for b in self.bitmaps.values() {
            if let Some((base, _)) = b.dib() {
                memory.check_unmap(base, b.span())?;
            }
        }
        Ok(())
    }
    pub fn clear_dibs(&self, memory: &mut Memory) {
        for b in self.bitmaps.values() {
            if let Some((base, _)) = b.dib() {
                memory
                    .unmap(base, b.span())
                    .expect("preflighted rejected DIB");
            }
        }
    }
    fn is_brush(&self, h: u64) -> bool {
        h >= self.brush && h <= self.brush + 20 && (h - self.brush).is_multiple_of(4)
    }
    pub(crate) fn upload(
        &mut self,
        window: u64,
        width: u32,
        height: u32,
        bgra: &[u32],
    ) -> Result<(), u32> {
        let count = pixels::dimensions(width, height)? as usize;
        if bgra.len() != count {
            return Err(87);
        }
        let existing = self.windows.get(&window).copied();
        self.capacity(width, height, existing)?;
        let handle = existing.map_or_else(|| self.handle(1), Ok)?;
        let mut data = pixels::allocate(width, height)?;
        for (dst, src) in data.iter_mut().zip(bgra) {
            *dst = layout::swap_rb(*src) & 0xffffff;
        }
        self.bitmaps.insert(
            handle,
            Bitmap {
                width,
                height,
                bpp: 32,
                storage: Storage::Owned(data),
                internal: true,
            },
        );
        if existing.is_none() {
            self.next += 16;
            self.windows.insert(window, handle);
        }
        Ok(())
    }
    pub fn snapshot(&self, window: u64) -> Result<Option<crate::machine::WindowPixels>, String> {
        let Some(bitmap) = self.windows.get(&window).and_then(|h| self.bitmaps.get(h)) else {
            return Ok(None);
        };
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(bitmap.owned().len() * 4)
            .map_err(|_| "pixel snapshot allocation failed")?;
        for c in bitmap.owned() {
            rgba.extend_from_slice(&(c | 0xff000000).to_le_bytes());
        }
        Ok(Some(crate::machine::WindowPixels {
            width: bitmap.width,
            height: bitmap.height,
            rgba,
        }))
    }
    pub fn brush_color(&self, brush: u64) -> Option<u32> {
        if brush >= self.brush && (brush - self.brush).is_multiple_of(4) {
            match (brush - self.brush) / 4 {
                0 => Some(0xffffff),
                1 => Some(0xc0c0c0),
                2 => Some(0x808080),
                3 => Some(0x404040),
                4 => Some(0),
                _ => None,
            }
        } else if (1..=31).contains(&brush) {
            Some(colors::system(brush as u32 - 1))
        } else {
            None
        }
    }
    pub fn erase(&mut self, window: u64, handle: u64, tid: u32, brush: u64) -> Result<u64, u32> {
        let info = self.dcs.information(handle)?;
        if info.0 != window || info.1 != u64::from(tid) || info.2 {
            return Err(6);
        }
        if brush == self.brush + 20 {
            return Ok(1);
        }
        let Some(color) = self.brush_color(brush) else {
            return Ok(0);
        };
        let dc = self.dcs.state(handle)?;
        let bitmap = dc.bitmap;
        let b = &self.bitmaps[&bitmap];
        let clip = dc.clip.unwrap_or([0, 0, b.width as i32, b.height as i32]);
        let b = self.bitmaps.get_mut(&bitmap).unwrap();
        for y in clip[1].max(0)..clip[3].min(b.height as i32) {
            for x in clip[0].max(0)..clip[2].min(b.width as i32) {
                let index = y as usize * b.width as usize + x as usize;
                b.owned_mut()[index] = color;
            }
        }
        Ok(1)
    }
    fn selected_elsewhere(&self, bitmap: u64, except: u64) -> bool {
        self.dcs
            .states()
            .any(|(h, d)| h != except && d.bitmap == bitmap)
    }
    fn bitmap(&self, handle: u64) -> Result<&Bitmap, u32> {
        self.bitmaps.get(&handle).ok_or(6)
    }
    fn point(&self, handle: u64, x: u64, y: u64) -> Result<(u64, u32, u32), u32> {
        let dc = self.dcs.state(handle)?;
        let b = self.bitmap(dc.bitmap)?;
        let x = i64::from(x as u32 as i32) - i64::from(dc.origin[0]) + i64::from(dc.viewport[0]);
        let y = i64::from(y as u32 as i32) - i64::from(dc.origin[1]) + i64::from(dc.viewport[1]);
        let clip = dc.clip.unwrap_or([0, 0, b.width as i32, b.height as i32]);
        if x < 0
            || y < 0
            || x >= i64::from(b.width)
            || y >= i64::from(b.height)
            || x < i64::from(clip[0])
            || y < i64::from(clip[1])
            || x >= i64::from(clip[2])
            || y >= i64::from(clip[3])
        {
            Err(87)
        } else {
            Ok((dc.bitmap, x as u32, y as u32))
        }
    }
}
pub(super) fn font_service(id: super::catalog::ApiId) -> bool {
    matches!(
        id,
        super::catalog::ApiId::GdiOperation(
            O::FontA | O::FontW | O::FontIndirectA | O::FontIndirectW | O::TextMetricsW
        )
    )
}
pub(super) fn implemented(id: super::catalog::ApiId) -> bool {
    use super::catalog::ApiId;
    matches!(
        id,
        ApiId::GdiDc(
            D::Window | D::Release | D::Compatible | D::Delete | D::Capabilities | D::PixelFormat
        ) | ApiId::GdiOperation(_)
    )
}
pub(super) fn dc(
    id: D,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: &mut crate::process::Loaded,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    let tid = image.threads.active;
    let result = match id {
        D::Window => {
            let (width, height) = if a[0] == 0 {
                (1280, 720)
            } else if let Some(w) = image.user.windows.get(&a[0]) {
                (
                    (w.client[2] - w.client[0]) as u32,
                    (w.client[3] - w.client[1]) as u32,
                )
            } else {
                return user::error(memory, state, 1400);
            };
            image.user.gdi.acquire(a[0], tid, width, height, None)
        }
        D::Release => image.user.gdi.release(a[0], a[1], tid, false),
        D::Compatible => {
            let g = &mut image.user.gdi;
            (if g.count() >= 4096 { Err(8) } else { Ok(()) })
                .and_then(|_| g.dcs.acquire(a[0], u64::from(tid), true, |_| false))
                .and_then(|handle| {
                    *g.dcs.state_mut(handle)? = Dc {
                        bitmap: g.stock,
                        background: 0xffffff,
                        brush: g.brush,
                        font: g.system_font,
                        ..Dc::default()
                    };
                    Ok(handle)
                })
        }
        D::Delete => image.user.gdi.dcs.release(a[0], 0, u64::from(tid), true),
        D::Capabilities | D::PixelFormat => image.user.gdi.dcs.validate(a[0]).map(|()| {
            if id == D::PixelFormat {
                0
            } else {
                match a[1] as u32 {
                    4 => (1280 * 254 + 480) / 960,
                    6 => (720 * 254 + 480) / 960,
                    8 | 118 => 1280,
                    10 | 117 => 720,
                    38 => 0x2001,
                    n => identities::capabilities64(n),
                }
            }
        }),
        D::WindowFrame => unreachable!(),
    };
    match result {
        Ok(v) => Ok(v),
        Err(code) => user::error(memory, state, code),
    }
}
pub(super) fn operation(
    id: O,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: &mut crate::process::Loaded,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    let g = &mut image.user.gdi;
    let result = (|| -> Result<u64, Failure> {
        if let Some(result) = font::dispatch(id, a, memory, g) {
            return result;
        }
        Ok(match id {
            O::Flush => 1,
            O::GetMapMode | O::SetMapMode => {
                g.dcs.validate(a[0])?;
                if id == O::SetMapMode && a[1] != 1 {
                    return Err(Failure::Windows(50));
                }
                1
            }
            O::CompatibleBitmap => {
                let source = g.bitmap(g.dcs.state(a[0])?.bitmap)?;
                let (width, height, bpp) = if a[1] == 0 || a[2] == 0 {
                    (1, 1, 1)
                } else {
                    if (a[1] as u32 as i32) < 0 || (a[2] as u32 as i32) < 0 {
                        return Err(Failure::Windows(87));
                    }
                    (a[1] as u32, a[2] as u32, source.bpp)
                };
                g.capacity(width, height, None)?;
                let handle = g.handle(1)?;
                let storage = if let Some((_, mut layout)) =
                    source.dib().filter(|_| a[1] != 0 && a[2] != 0)
                {
                    layout.width = width;
                    layout.height = height;
                    Storage::Dib {
                        base: memory.map_dib(u64::from(width) * u64::from(height) * 4)?,
                        layout,
                    }
                } else {
                    Storage::Owned(pixels::allocate(width, height)?)
                };
                g.bitmaps.insert(
                    handle,
                    Bitmap {
                        width,
                        height,
                        bpp,
                        storage,
                        internal: false,
                    },
                );
                g.next += 16;
                handle
            }
            O::Select => {
                if g.is_brush(a[1]) {
                    return Ok(std::mem::replace(&mut g.dcs.state_mut(a[0])?.brush, a[1]));
                }
                if !g.dcs.information(a[0])?.2 {
                    return Err(Failure::Windows(87));
                }
                let bitmap = g.bitmap(a[1])?;
                if bitmap.internal && a[1] != g.stock {
                    return Err(Failure::Windows(87));
                }
                if a[1] != g.stock && g.selected_elsewhere(a[1], a[0]) {
                    return Err(Failure::Windows(170));
                }
                std::mem::replace(&mut g.dcs.state_mut(a[0])?.bitmap, a[1])
            }
            O::DibSection | O::GetDib | O::StretchDib => dib::dispatch(id, a, memory, g)?,
            O::Delete => {
                if g.is_brush(a[0]) {
                    return Ok(1);
                }
                let b = g.bitmap(a[0])?;
                if b.internal || g.selected_elsewhere(a[0], 0) {
                    return Err(Failure::Windows(170));
                }
                if let Some((base, _)) = b.dib() {
                    memory
                        .unmap(base, b.span())
                        .map_err(|_| Failure::Windows(487))?;
                }
                g.bitmaps.remove(&a[0]);
                1
            }
            O::ObjectA | O::ObjectW => {
                if g.is_brush(a[0]) {
                    let size = if memory.mode() == Mode::X86 { 12 } else { 16 };
                    if a[2] == 0 {
                        return Ok(size as u64);
                    }
                    if (a[1] as u32 as i32) < size as i32 {
                        return Err(Failure::Windows(87));
                    }
                    let mut out = vec![0; size];
                    out[..4].copy_from_slice(&u32::from(a[0] == g.brush + 20).to_le_bytes());
                    out[4..8].copy_from_slice(&g.brush_color(a[0]).unwrap_or(0).to_le_bytes());
                    memory.write(a[2], &out)?;
                    return Ok(size as u64);
                }
                dib::object(g.bitmap(a[0])?, a, memory)?
            }
            O::ObjectType => {
                if g.is_brush(a[0]) {
                    2
                } else if g.bitmaps.contains_key(&a[0]) {
                    7
                } else if let Ok((_, _, compatible)) = g.dcs.information(a[0]) {
                    if compatible {
                        10
                    } else {
                        3
                    }
                } else {
                    0
                }
            }
            O::CurrentObject => {
                if a[1] == 2 {
                    return Ok(g.dcs.state(a[0])?.brush);
                }
                if a[1] != 7 {
                    return Err(Failure::Windows(50));
                }
                g.dcs.state(a[0])?.bitmap
            }
            O::StockObject => match a[0] {
                21 => g.stock,
                0..=5 => g.brush + a[0] * 4,
                _ => return Err(Failure::Windows(50)),
            },
            O::SetPixel | O::SetPixelV | O::GetPixel => {
                let (bitmap, x, y) = g.point(a[0], a[1], a[2])?;
                let b = g.bitmaps.get_mut(&bitmap).unwrap();
                if id == O::GetPixel {
                    u64::from(b.read(memory, x, y)?)
                } else {
                    if a[3] & 0xff000000 != 0 {
                        return Err(Failure::Windows(50));
                    }
                    let value = b.write(memory, x, y, a[3] as u32)?;
                    if id == O::SetPixelV {
                        1
                    } else {
                        u64::from(value)
                    }
                }
            }
            O::Blit => {
                if a[8] != 0xcc0020 {
                    return Err(Failure::Windows(50));
                }
                let d = g.dcs.state(a[0])?;
                let source = g.dcs.state(a[5])?;
                let target = d.bitmap;
                let source_id = source.bitmap;
                let point = |dc: &Dc, x: u64, y: u64| {
                    [
                        i64::from(x as u32 as i32) - i64::from(dc.origin[0])
                            + i64::from(dc.viewport[0]),
                        i64::from(y as u32 as i32) - i64::from(dc.origin[1])
                            + i64::from(dc.viewport[1]),
                    ]
                };
                let [dx, dy] = point(d, a[1], a[2]);
                let [sx, sy] = point(source, a[6], a[7]);
                let width = i64::from(a[3] as u32 as i32);
                let height = i64::from(a[4] as u32 as i32);
                if width < 0 || height < 0 {
                    return Err(Failure::Windows(87));
                }
                let dst = g.bitmap(target)?;
                let clip = d
                    .clip
                    .unwrap_or([0, 0, dst.width as i32, dst.height as i32]);
                let x0 = 0.max(i64::from(clip[0].max(0)) - dx);
                let y0 = 0.max(i64::from(clip[1].max(0)) - dy);
                let x1 = width.min(i64::from(clip[2].min(dst.width as i32)) - dx);
                let y1 = height.min(i64::from(clip[3].min(dst.height as i32)) - dy);
                if x1 <= x0 || y1 <= y0 {
                    return Ok(1);
                }
                let src = g.bitmap(source_id)?;
                if sx + x0 < 0
                    || sy + y0 < 0
                    || sx + x1 > i64::from(src.width)
                    || sy + y1 > i64::from(src.height)
                {
                    return Err(Failure::Windows(87));
                }
                let sr = [
                    (sx + x0) as u32,
                    (sy + y0) as u32,
                    (sx + x1) as u32,
                    (sy + y1) as u32,
                ];
                let dr = [
                    (dx + x0) as u32,
                    (dy + y0) as u32,
                    (dx + x1) as u32,
                    (dy + y1) as u32,
                ];
                src.check_rect(memory, sr, false)?;
                dst.check_rect(memory, dr, true)?;
                let values = src.read_rect(memory, sr)?;
                let dst = g.bitmaps.get_mut(&target).unwrap();
                dst.write_rect(memory, dr, &values)?;
                1
            }
            O::SetWindowOrigin
            | O::SetViewportOrigin
            | O::GetWindowOrigin
            | O::GetViewportOrigin => {
                let dc = g.dcs.state_mut(a[0])?;
                let slot = if matches!(id, O::SetWindowOrigin | O::GetWindowOrigin) {
                    &mut dc.origin
                } else {
                    &mut dc.viewport
                };
                let setting = matches!(id, O::SetWindowOrigin | O::SetViewportOrigin);
                let out = if setting { a[3] } else { a[1] };
                if out == 0 && !setting {
                    return Err(Failure::Windows(87));
                }
                if out != 0 {
                    memory.write(
                        out,
                        &slot
                            .iter()
                            .flat_map(|v| v.to_le_bytes())
                            .collect::<Vec<_>>(),
                    )?;
                }
                if setting {
                    *slot = [a[1] as u32 as i32, a[2] as u32 as i32];
                }
                1
            }
            O::SetTextColor | O::GetTextColor | O::SetBackgroundColor | O::GetBackgroundColor => {
                let dc = g.dcs.state_mut(a[0])?;
                let slot = if matches!(id, O::SetTextColor | O::GetTextColor) {
                    &mut dc.text
                } else {
                    &mut dc.background
                };
                let old = *slot;
                if matches!(id, O::SetTextColor | O::SetBackgroundColor) {
                    if a[1] & 0xff000000 != 0 {
                        return Err(Failure::Windows(50));
                    }
                    *slot = a[1] as u32;
                }
                u64::from(old)
            }
            _ => unreachable!(),
        })
    })();
    match result {
        Ok(v) => Ok(v),
        Err(Failure::Fault(f)) => Err(ServiceError::Fault(f)),
        Err(Failure::Windows(code)) => {
            user::error(memory, state, code)?;
            Ok(
                if matches!(
                    id,
                    O::GetPixel
                        | O::SetPixel
                        | O::SetTextColor
                        | O::GetTextColor
                        | O::SetBackgroundColor
                        | O::GetBackgroundColor
                ) {
                    u64::from(u32::MAX)
                } else {
                    0
                },
            )
        }
    }
}
enum Failure {
    Windows(u32),
    Fault(crate::memory::Fault),
}
impl From<u32> for Failure {
    fn from(v: u32) -> Self {
        Self::Windows(v)
    }
}
impl From<crate::memory::Fault> for Failure {
    fn from(v: crate::memory::Fault) -> Self {
        Self::Fault(v)
    }
}

#[cfg(test)]
mod resize_tests {
    use super::*;

    #[test]
    fn window_resize_batch_checks_total_budget_before_allocating_and_keeps_dc_identity() {
        let mut gdi = Gdi::new(Mode::X86);
        let dcs: Vec<_> = (1..=4)
            .map(|w| gdi.acquire(w, 1, 2, 2, None).unwrap())
            .collect();
        let handles: Vec<_> = dcs
            .iter()
            .map(|&dc| gdi.dcs.state(dc).unwrap().bitmap)
            .collect();
        for &handle in &handles {
            gdi.bitmaps
                .get_mut(&handle)
                .unwrap()
                .owned_mut()
                .copy_from_slice(&[1, 2, 3, 4]);
        }
        // Each replacement is within the per-bitmap limit; together with the
        // stock bitmap they exceed the process budget. No large allocation runs.
        let requests: Vec<_> = (1..=4).map(|w| (w, 8192, 2048)).collect();
        assert!(matches!(gdi.prepare_resizes(&requests), Err(8)));
        for &handle in &handles {
            let b = &gdi.bitmaps[&handle];
            assert_eq!((b.width, b.height, b.owned()), (2, 2, &[1, 2, 3, 4][..]));
        }
        let prepared = gdi.prepare_resizes(&[(1, 3, 3), (2, 1, 1)]).unwrap();
        assert_eq!(gdi.bitmaps[&handles[0]].width, 2);
        gdi.commit_resizes(prepared);
        for (&dc, &handle) in dcs.iter().zip(&handles) {
            assert_eq!(gdi.dcs.state(dc).unwrap().bitmap, handle);
        }
        assert_eq!(
            gdi.bitmaps[&handles[0]].owned(),
            &[1, 2, 0, 3, 4, 0, 0, 0, 0]
        );
        assert_eq!(gdi.bitmaps[&handles[1]].owned(), &[1]);
    }
}
