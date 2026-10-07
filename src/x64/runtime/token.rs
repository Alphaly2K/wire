//! Query-only identity of wire's ordinary, confined guest user. No host credentials or elevation.
pub(super) fn sid(mem: &super::Memory, address: u64) -> Result<Vec<u8>, u32> {
    let mut header = [0; 8];
    mem.read(address, &mut header).map_err(|_| 1337u32)?;
    if header[0] != 1 || header[1] > 15 {
        return Err(1337);
    }
    let mut data = vec![0; 8 + header[1] as usize * 4];
    mem.read(address, &mut data).map_err(|_| 1337u32)?;
    Ok(data)
}
pub(super) fn information(class: u32, output: u64) -> Result<Vec<u8>, u32> {
    let data = match class {
        18 => 1u32.to_le_bytes().to_vec(), // TokenElevationTypeDefault: no linked elevated token.
        20 | 29 => 0u32.to_le_bytes().to_vec(),
        8 => 1u32.to_le_bytes().to_vec(), // TokenPrimary
        1 => {
            let mut bytes = vec![0; 44];
            bytes[..8].copy_from_slice(&output.checked_add(16).ok_or(87u32)?.to_le_bytes());
            bytes[16..24].copy_from_slice(&[1, 5, 0, 0, 0, 0, 0, 5]);
            for (n, part) in [21u32, 1, 1, 1, 1000].iter().enumerate() {
                bytes[24 + n * 4..28 + n * 4].copy_from_slice(&part.to_le_bytes());
            }
            bytes
        }
        25 => {
            let mut bytes = vec![0; 28];
            bytes[..8].copy_from_slice(&output.checked_add(16).ok_or(87u32)?.to_le_bytes());
            bytes[8..12].copy_from_slice(&0x60u32.to_le_bytes());
            bytes[16..24].copy_from_slice(&[1, 1, 0, 0, 0, 0, 0, 16]);
            bytes[24..28].copy_from_slice(&8192u32.to_le_bytes());
            bytes
        }
        _ => return Err(50),
    };
    // Keep structure padding zero; guest pointers are filled only for pointer-bearing classes.
    Ok(data)
}
