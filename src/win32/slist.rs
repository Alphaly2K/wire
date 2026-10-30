//! Windows SLIST depth and sequence fields wrap independently of link bits.
pub(crate) fn update32(previous: u64, next: u32, depth: u16) -> u64 {
    let sequence = ((previous >> 48) as u16).wrapping_add(1);
    u64::from(next) | (u64::from(depth) << 32) | (u64::from(sequence) << 48)
}

pub(crate) fn update64(packed: u64, region: u64, next: u64, depth: u16) -> [u8; 16] {
    let mut header = [0; 16];
    header[..8].copy_from_slice(
        &((packed.wrapping_add(1 << 16) & !0xffff) | u64::from(depth)).to_le_bytes(),
    );
    header[8..].copy_from_slice(&(next | (region & 15)).to_le_bytes());
    header
}
