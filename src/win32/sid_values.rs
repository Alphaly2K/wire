//! SID layout without guest memory or execution state.
pub(crate) const fn length(count: u8) -> usize {
    8 + count as usize * 4
}
pub(crate) fn valid(header: &[u8]) -> bool {
    header[0] == 1 && header[1] <= 15
}
pub(crate) fn header(count: u8, authority: [u8; 6]) -> [u8; 8] {
    let mut bytes = [1, count, 0, 0, 0, 0, 0, 0];
    bytes[2..].copy_from_slice(&authority);
    bytes
}
