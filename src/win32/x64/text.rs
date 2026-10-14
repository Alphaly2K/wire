//! Checked Win64 raw string transfer; HRESULT policy belongs to the API adapter.
use crate::x64::memory::Memory;
pub(crate) fn utf16(mem: &Memory, ptr: u64) -> Result<String, String> {
    let mut units = Vec::new();
    for n in 0..32768u64 {
        let u = mem.read_int(ptr.checked_add(n * 2).ok_or("string overflow")?, 2)? as u16;
        if u == 0 {
            return String::from_utf16(&units).map_err(|_| "invalid Win64 UTF-16 string".into());
        }
        units.push(u);
    }
    Err("unterminated Win64 UTF-16 string".into())
}
pub(crate) fn string_bytes(
    mem: &Memory,
    ptr: u64,
    count: u64,
    width: usize,
) -> Result<Vec<u8>, String> {
    let count = count as u32 as i32;
    if count == 0 || count < -1 || ptr == 0 {
        return Err("invalid Win64 string input".into());
    }
    let length = if count == -1 {
        let mut n = 0;
        loop {
            if n >= 32768 {
                return Err("Win64 terminated string exceeds capacity".into());
            }
            let unit = mem.read_int(
                ptr.checked_add(n as u64 * width as u64)
                    .ok_or("string overflow")?,
                width,
            )?;
            n += 1;
            if unit == 0 {
                break n;
            }
        }
    } else {
        count as usize
    };
    let size = length
        .checked_mul(width)
        .filter(|&n| n <= 16 * 1024 * 1024)
        .ok_or("Win64 string transfer exceeds capacity")?;
    let mut bytes = vec![0; size];
    mem.read(ptr, &mut bytes)?;
    Ok(bytes)
}
