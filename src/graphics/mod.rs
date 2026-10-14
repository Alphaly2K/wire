//! Initial GDI object/pixel backend. Public snapshots are real CPU pixels for
//! independent fixtures; platform presentation and drawing APIs are separate.
mod dib;
mod font;
mod operations;
mod surface;
use crate::mem::GuestMem;
pub(crate) use operations::execute as execute_gdi;
use std::collections::BTreeMap;
use surface::{dimensions, Bitmap, Storage};
type Arguments = [u32; 16];

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
