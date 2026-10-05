//! Pixel storage used by the initial checked GDI raster backend.
//! DIB pixels remain in guest memory; no host pointer survives an API call.
use crate::mem::GuestMem;

pub(super) enum Storage {
    Owned(Vec<u32>),                   // COLORREF, 0x00BBGGRR
    Dib { base: u32, top_down: bool }, // BGRA8, alpha retained on direct writes
}
pub(super) struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub bpp: u32,
    pub storage: Storage,
    pub internal: bool,
}
impl Bitmap {
    pub fn owned(width: u32, height: u32, bpp: u32, internal: bool) -> Result<Self, u32> {
        let count = dimensions(width, height)?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(count as usize).map_err(|_| 8u32)?;
        pixels.resize(count as usize, 0);
        Ok(Self {
            width,
            height,
            bpp,
            storage: Storage::Owned(pixels),
            internal,
        })
    }
    pub fn size(&self) -> u32 {
        self.width * self.height * 4
    }
    pub fn stride(&self) -> u32 {
        if matches!(self.storage, Storage::Dib { .. }) {
            self.width * 4
        } else {
            (self.width * self.bpp).div_ceil(16) * 2
        }
    }
    fn offset(&self, x: u32, y: u32) -> u32 {
        let y = if matches!(
            self.storage,
            Storage::Dib {
                top_down: false,
                ..
            }
        ) {
            self.height - 1 - y
        } else {
            y
        };
        (y * self.width + x) * 4
    }
    pub fn dib_base(&self) -> Option<u32> {
        match self.storage {
            Storage::Dib { base, .. } => Some(base),
            _ => None,
        }
    }
    pub fn check(&self, mem: &GuestMem, write: bool) -> Result<(), u32> {
        if let Some(base) = self.dib_base() {
            mem.ptr(base, self.size(), write).map_err(|_| 87u32)?;
        }
        Ok(())
    }
    pub fn read(&self, mem: &GuestMem, x: u32, y: u32) -> Result<u32, u32> {
        if x >= self.width || y >= self.height {
            return Err(87);
        }
        match &self.storage {
            Storage::Owned(pixels) => Ok(pixels[(y * self.width + x) as usize]),
            Storage::Dib { base, .. } => {
                let bgra = mem.read_u32(base + self.offset(x, y)).map_err(|_| 87u32)?;
                Ok(swap_rb(bgra) & 0xffffff)
            }
        }
    }
    pub fn write(&mut self, mem: &GuestMem, x: u32, y: u32, color: u32) -> Result<u32, u32> {
        if x >= self.width || y >= self.height {
            return Err(87);
        }
        let mut color = color & 0xffffff;
        if self.bpp == 1 {
            let sum = (color & 255) + ((color >> 8) & 255) + ((color >> 16) & 255);
            color = if sum > 382 { 0xffffff } else { 0 };
        }
        let offset = self.offset(x, y);
        match &mut self.storage {
            Storage::Owned(pixels) => pixels[(y * self.width + x) as usize] = color,
            Storage::Dib { base, .. } => mem
                .write_u32(*base + offset, swap_rb(color))
                .map_err(|_| 87u32)?,
        }
        Ok(color)
    }
}
pub(super) fn dimensions(width: u32, height: u32) -> Result<u32, u32> {
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(87);
    }
    let count = width.checked_mul(height).ok_or(87u32)?;
    if count > 16 * 1024 * 1024 {
        return Err(8);
    }
    Ok(count)
}
pub(super) fn swap_rb(c: u32) -> u32 {
    (c & 0xff00ff00) | ((c & 255) << 16) | ((c >> 16) & 255)
}
