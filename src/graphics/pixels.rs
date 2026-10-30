//! Bounded owned COLORREF pixels, shared by the guest memory adapters.
pub(crate) fn dimensions(width: u32, height: u32) -> Result<u32, u32> {
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err(87);
    }
    let count = width.checked_mul(height).ok_or(87u32)?;
    if count > 16 * 1024 * 1024 {
        return Err(8);
    }
    Ok(count)
}
pub(crate) fn allocate(width: u32, height: u32) -> Result<Vec<u32>, u32> {
    let count = dimensions(width, height)?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(count as usize).map_err(|_| 8u32)?;
    pixels.resize(count as usize, 0);
    Ok(pixels)
}
pub(crate) fn color(value: u32, bpp: u32) -> u32 {
    let value = value & 0xffffff;
    if bpp == 1 {
        if (value & 255) + ((value >> 8) & 255) + ((value >> 16) & 255) > 382 {
            0xffffff
        } else {
            0
        }
    } else {
        value
    }
}
