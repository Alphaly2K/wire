//! Initial GDI object/pixel backend. Public snapshots are real CPU pixels for
//! independent fixtures; platform presentation and drawing APIs are separate.
mod dib;
mod font;
mod surface;
use crate::{api::ApiState, mem::GuestMem};
use std::collections::BTreeMap;
use surface::{dimensions, Bitmap, Storage};

const STOCK_BITMAP: u32 = 0x42000004;
struct Dc {
    window: Option<u32>, // None = compatible memory DC
    bitmap: u32,
    origin: [i32; 2],
    viewport: [i32; 2],
    text_color: u32,
    bk_color: u32,
    font: u32,
    clip: Option<[i32; 4]>,
}
impl Dc {
    fn new(window: Option<u32>, bitmap: u32) -> Self {
        Self {
            window,
            bitmap,
            origin: [0; 2],
            viewport: [0; 2],
            text_color: 0,
            bk_color: 0xffffff,
            font: font::SYSTEM_FONT,
            clip: None,
        }
    }
    fn point(&self, x: u32, y: u32) -> [i64; 2] {
        let point = [x, y];
        std::array::from_fn(|i| {
            point[i] as i32 as i64 - self.origin[i] as i64 + self.viewport[i] as i64
        })
    }
    fn bounds(&self, width: u32, height: u32) -> [i64; 4] {
        let r = self.clip.unwrap_or([0, 0, width as i32, height as i32]);
        [
            r[0].max(0) as i64,
            r[1].max(0) as i64,
            r[2].min(width as i32) as i64,
            r[3].min(height as i32) as i64,
        ]
    }
}
pub struct Graphics {
    dcs: BTreeMap<u32, Dc>,
    bitmaps: BTreeMap<u32, Bitmap>,
    windows: BTreeMap<u32, u32>,
    next: u32,
    fonts: BTreeMap<u32, font::Font>,
}
impl Default for Graphics {
    fn default() -> Self {
        Self {
            dcs: BTreeMap::new(),
            bitmaps: BTreeMap::from([(STOCK_BITMAP, Bitmap::owned(1, 1, 1, true).unwrap())]),
            windows: BTreeMap::new(),
            next: 0x42010000,
            fonts: font::stock_fonts(),
        }
    }
}
impl Graphics {
    /// Upload checked top-down D3D pixels to the same surface read by GDI.
    pub(crate) fn upload(
        &mut self,
        hwnd: u32,
        width: u32,
        height: u32,
        bgra: &[u32],
    ) -> Result<(), u32> {
        if bgra.len() != dimensions(width, height)? as usize {
            return Err(87);
        }
        let dc = self.window_dc(hwnd, width, height)?;
        let bitmap = self.dcs[&dc].bitmap;
        if let Storage::Owned(pixels) = &mut self.bitmaps.get_mut(&bitmap).unwrap().storage {
            for (dst, &src) in pixels.iter_mut().zip(bgra) {
                *dst = surface::swap_rb(src) & 0xffffff;
            }
        }
        self.release_window_dc(hwnd, dc)
    }
    fn handle(&mut self) -> Result<u32, u32> {
        if self.dcs.len() + self.bitmaps.len() + self.fonts.len() >= 4096 {
            return Err(8);
        }
        let h = self.next;
        self.next = h.checked_add(4).ok_or(8u32)?;
        Ok(h)
    }
    fn capacity(&self, width: u32, height: u32) -> Result<(), u32> {
        let size = dimensions(width, height)? as u64 * 4;
        let used: u64 = self.bitmaps.values().map(|b| b.size() as u64).sum();
        if used + size > 256 * 1024 * 1024 {
            return Err(8);
        }
        Ok(())
    }
    fn dc(&self, h: u32) -> Result<&Dc, u32> {
        self.dcs.get(&h).ok_or(6)
    }
    pub(crate) fn window_dc(&mut self, hwnd: u32, width: u32, height: u32) -> Result<u32, u32> {
        let dc = self.handle()?;
        let bitmap = if let Some(&h) = self.windows.get(&hwnd) {
            let old = &self.bitmaps[&h];
            if old.width != width || old.height != height {
                self.capacity(width, height)?;
                let mut new = Bitmap::owned(width, height, 32, true)?;
                if let (Storage::Owned(src), Storage::Owned(dst)) = (&old.storage, &mut new.storage)
                {
                    for y in 0..height.min(old.height) {
                        let count = width.min(old.width) as usize;
                        dst[y as usize * width as usize..y as usize * width as usize + count]
                            .copy_from_slice(
                                &src[y as usize * old.width as usize
                                    ..y as usize * old.width as usize + count],
                            );
                    }
                }
                self.bitmaps.insert(h, new);
            }
            h
        } else {
            self.capacity(width, height)?;
            let h = self.handle()?;
            self.bitmaps
                .insert(h, Bitmap::owned(width, height, 32, true)?);
            self.windows.insert(hwnd, h);
            h
        };
        self.dcs.insert(dc, Dc::new(Some(hwnd), bitmap));
        Ok(dc)
    }
    pub(crate) fn release_window_dc(&mut self, hwnd: u32, dc: u32) -> Result<(), u32> {
        if self.dc(dc)?.window != Some(hwnd) {
            return Err(6);
        }
        self.dcs.remove(&dc);
        Ok(())
    }
    pub(crate) fn set_clip(&mut self, dc: u32, rect: [i32; 4]) -> Result<(), u32> {
        self.dcs.get_mut(&dc).ok_or(6u32)?.clip = Some(rect);
        Ok(())
    }
    pub(crate) fn fill_color(&mut self, mem: &GuestMem, dc: u32, color: u32) -> Result<(), u32> {
        let dc = self.dc(dc)?;
        let b = &self.bitmaps[&dc.bitmap];
        let r = dc.bounds(b.width, b.height);
        let h = dc.bitmap;
        let bitmap = self.bitmaps.get_mut(&h).unwrap();
        bitmap.check(mem, true)?;
        for y in r[1]..r[3] {
            for x in r[0]..r[2] {
                bitmap.write(mem, x as u32, y as u32, color)?;
            }
        }
        Ok(())
    }
    pub fn window_snapshot(&self, mem: &GuestMem, hwnd: u32) -> Result<(u32, u32, Vec<u32>), u32> {
        let bitmap = self
            .bitmaps
            .get(self.windows.get(&hwnd).ok_or(6u32)?)
            .ok_or(6u32)?;
        bitmap.check(mem, false)?;
        let mut pixels = Vec::with_capacity((bitmap.width * bitmap.height) as usize);
        for y in 0..bitmap.height {
            for x in 0..bitmap.width {
                pixels.push(bitmap.read(mem, x, y)?);
            }
        }
        Ok((bitmap.width, bitmap.height, pixels))
    }
    pub(crate) fn remove_window(&mut self, hwnd: u32) {
        self.dcs.retain(|_, dc| dc.window != Some(hwnd));
        if let Some(h) = self.windows.remove(&hwnd) {
            self.bitmaps.remove(&h);
        }
    }
    /// Snapshot logical top-down pixels as COLORREFs. Used by fixture export
    /// and the future presentation interface; each DIB access stays checked.
    pub fn snapshot(&self, mem: &GuestMem, dc: u32) -> Result<(u32, u32, Vec<u32>), u32> {
        let bitmap = self.bitmaps.get(&self.dc(dc)?.bitmap).ok_or(6u32)?;
        bitmap.check(mem, false)?;
        let mut pixels = Vec::with_capacity((bitmap.width * bitmap.height) as usize);
        for y in 0..bitmap.height {
            for x in 0..bitmap.width {
                pixels.push(bitmap.read(mem, x, y)?);
            }
        }
        Ok((bitmap.width, bitmap.height, pixels))
    }
}
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
fn bitmap_object(mem: &GuestMem, bitmap: &Bitmap, a: &crate::api::Args) -> Result<u32, u32> {
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
            0,
            bitmap.size(),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
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
fn blit(a: &crate::api::Args, mem: &GuestMem, gfx: &mut Graphics) -> Result<u32, u32> {
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
pub(crate) fn dispatch(
    dll: &str,
    name: &str,
    a: &crate::api::Args,
    mem: &mut GuestMem,
    state: &mut ApiState,
) -> Option<(u32, String, bool)> {
    let user = dll == "user32.dll" && matches!(name, "GetDC" | "ReleaseDC");
    let gdi = dll == "gdi32.dll"
        && matches!(
            name,
            "CreateCompatibleDC"
                | "DeleteDC"
                | "CreateCompatibleBitmap"
                | "CreateDIBSection"
                | "DeleteObject"
                | "SelectObject"
                | "GetObjectA"
                | "GetObjectW"
                | "GetObjectType"
                | "GetCurrentObject"
                | "GetStockObject"
                | "SetPixel"
                | "SetPixelV"
                | "GetPixel"
                | "BitBlt"
                | "SetWindowOrgEx"
                | "SetViewportOrgEx"
                | "GetWindowOrgEx"
                | "GetViewportOrgEx"
                | "SetTextColor"
                | "GetTextColor"
                | "SetBkColor"
                | "GetBkColor"
                | "GdiFlush"
                | "StretchDIBits"
                | "GetDIBits"
                | "GetDeviceCaps"
                | "CreateFontA"
                | "CreateFontW"
                | "CreateFontIndirectA"
                | "CreateFontIndirectW"
        );
    if !user && !gdi {
        return None;
    }
    let result = (|| -> Result<u32, u32> {
        let gfx = &mut state.graphics;
        if let Some(result) = font::dispatch(name, a, mem, gfx) {
            return result;
        }
        if let Some(result) = dib::dispatch(name, a, mem, gfx) {
            return result;
        }
        match name {
            "GetDC" => {
                let (width, height) = if a[0] == 0 {
                    (1024, 768)
                } else {
                    let w = state.user32.windows.get(&a[0]).ok_or(1400u32)?;
                    let [width, height] = w.client_size();
                    (width.max(1) as u32, height.max(1) as u32)
                };
                gfx.window_dc(a[0], width, height)
            }
            "ReleaseDC" => {
                gfx.release_window_dc(a[0], a[1])?;
                Ok(1)
            }
            "CreateCompatibleDC" => {
                if a[0] != 0 {
                    gfx.dc(a[0])?;
                }
                let h = gfx.handle()?;
                gfx.dcs.insert(h, Dc::new(None, STOCK_BITMAP));
                Ok(h)
            }
            "DeleteDC" => {
                if gfx.dc(a[0])?.window.is_some() {
                    return Err(6);
                }
                gfx.dcs.remove(&a[0]);
                Ok(1)
            }
            "CreateCompatibleBitmap" => {
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
            "CreateDIBSection" => {
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
                if f[0] != 40 || f[3] != 0x00200001 || f[4] != 0 || f[8] != 0 {
                    return Err(120);
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
                        storage: Storage::Dib {
                            base,
                            top_down: height < 0,
                        },
                        internal: false,
                    },
                );
                Ok(h)
            }
            "SelectObject" => {
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
            "DeleteObject" => {
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
            "GetObjectA" | "GetObjectW" => {
                bitmap_object(mem, gfx.bitmaps.get(&a[0]).ok_or(6u32)?, a)
            }
            "GetObjectType" => Ok(if gfx.bitmaps.contains_key(&a[0]) {
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
            "GetCurrentObject" => {
                if a[1] != 7 {
                    return Err(120);
                }
                Ok(gfx.dc(a[0])?.bitmap)
            }
            "GetStockObject" => {
                if a[0] != 21 {
                    return Err(120);
                }
                Ok(STOCK_BITMAP)
            }
            "GetPixel" | "SetPixel" | "SetPixelV" => {
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
                if name == "GetPixel" {
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
                    Ok(if name == "SetPixelV" { 1 } else { color })
                }
            }
            "BitBlt" => blit(a, mem, gfx),
            "SetWindowOrgEx" | "SetViewportOrgEx" | "GetWindowOrgEx" | "GetViewportOrgEx" => {
                gfx.dc(a[0])?;
                let dc = gfx.dcs.get_mut(&a[0]).unwrap();
                let slot = if name.contains("Window") {
                    &mut dc.origin
                } else {
                    &mut dc.viewport
                };
                if name.starts_with("Set") {
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
            "SetTextColor" | "GetTextColor" | "SetBkColor" | "GetBkColor" => {
                gfx.dc(a[0])?;
                let dc = gfx.dcs.get_mut(&a[0]).unwrap();
                let slot = if name.contains("Text") {
                    &mut dc.text_color
                } else {
                    &mut dc.bk_color
                };
                let old = *slot;
                if name.starts_with("Set") {
                    if a[1] & 0xff000000 != 0 {
                        return Err(120);
                    }
                    *slot = a[1];
                }
                Ok(old)
            }
            "GdiFlush" => Ok(1), // all implemented operations are synchronous
            _ => unreachable!(),
        }
    })();
    Some(match result {
        Ok(v) => (v, "checked GDI object / CPU pixels".into(), true),
        Err(e) => {
            state.last_error = e;
            let v = if matches!(
                name,
                "GetPixel"
                    | "SetPixel"
                    | "SetTextColor"
                    | "GetTextColor"
                    | "SetBkColor"
                    | "GetBkColor"
            ) {
                u32::MAX
            } else {
                0
            };
            (v, format!("GDI error {e}"), e != 120)
        }
    })
}
