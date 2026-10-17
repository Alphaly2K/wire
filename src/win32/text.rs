//! Existing PE32 bounded UTF-16 reads shared by string and COM adapters.
use crate::mem::GuestMem;

pub(crate) fn units32(mem: &GuestMem, pointer: u32, length: u32) -> Result<Vec<u16>, u32> {
    if pointer == 0 {
        return Err(87);
    }
    if length == u32::MAX {
        return Ok(crate::menu::read_text(mem, pointer, true)?.unwrap_or_default());
    }
    if length > 1 << 20 {
        return Err(87);
    }
    let mut bytes = vec![0; length as usize * 2];
    mem.read_bytes(pointer, &mut bytes).map_err(|_| 87u32)?;
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect())
}
