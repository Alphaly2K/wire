//! Fixed user-mode CPU identity. Queries do not expose host capabilities.
pub const CLOCK_FREQUENCY: u32 = 10_000_000;

const ECX: u32 = (1 << 0) | (1 << 9) | (1 << 13) | (1 << 19) | (1 << 20) | (1 << 23);
const EDX: u32 =
    (1 << 0) | (1 << 4) | (1 << 8) | (1 << 15) | (1 << 23) | (1 << 24) | (1 << 25) | (1 << 26);

pub(crate) const LEAVES: [(u32, Option<u32>, [u32; 4]); 11] = [
    (
        0,
        None,
        [
            0x15,
            u32::from_le_bytes(*b"Wire"),
            u32::from_le_bytes(*b"CPU "),
            u32::from_le_bytes(*b"Virt"),
        ],
    ),
    (1, None, [0x600, 0x10000, ECX, EDX]),
    // FCS/FDS selectors are deprecated in all FP context formats.
    (7, Some(0), [0, 1 << 13, 0, 0]),
    (0x15, None, [1, 1, CLOCK_FREQUENCY, 0]),
    (0x80000000, None, [0x80000008, 0, 0, 0]),
    (
        0x80000001,
        None,
        [0, 0, 1, (1 << 20) | (1 << 27) | (1 << 29)],
    ),
    (
        0x80000002,
        None,
        [
            u32::from_le_bytes(*b"Wire"),
            u32::from_le_bytes(*b" Was"),
            u32::from_le_bytes(*b"m Vi"),
            u32::from_le_bytes(*b"rtua"),
        ],
    ),
    (
        0x80000003,
        None,
        [
            u32::from_le_bytes(*b"l CP"),
            u32::from_le_bytes(*b"U   "),
            u32::from_le_bytes(*b"    "),
            u32::from_le_bytes(*b"    "),
        ],
    ),
    (0x80000004, None, [u32::from_le_bytes(*b"    "); 4]),
    (0x80000007, None, [0, 0, 0, 1 << 8]),
    (0x80000008, None, [0x3030, 0, 0, 0]),
];

/// Returns EAX/EBX/ECX/EDX. Unpublished leaves and subleaves return zero.
pub fn cpuid(leaf: u32, subleaf: u32) -> [u32; 4] {
    LEAVES
        .iter()
        .find(|(number, sub, _)| *number == leaf && sub.is_none_or(|n| n == subleaf))
        .map_or([0; 4], |(_, _, value)| *value)
}
