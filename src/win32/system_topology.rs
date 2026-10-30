//! Windows relation records for one virtual processor, NUMA node and group.
pub(crate) fn encode(relation: u32, wide: bool) -> Option<Vec<u8>> {
    if !matches!(relation, 0 | 1 | 2 | 3 | 4 | 6 | 0xffff) {
        return None;
    }
    let affinity = if wide { 16 } else { 12 };
    let mut output = Vec::new();
    // CPUID advertises no hardware cache. RelationAll uses the original NUMA
    // record; the extended NUMA form is returned only when requested.
    for kind in [0u32, 1, 3, 4, 6] {
        if kind != relation && !(relation == 0xffff && kind != 6) {
            continue;
        }
        let size = if kind == 4 {
            72 + if wide { 8 } else { 4 }
        } else {
            32 + affinity
        };
        let mut record = vec![0; size];
        record[..4].copy_from_slice(&kind.to_le_bytes());
        record[4..8].copy_from_slice(&(size as u32).to_le_bytes());
        if kind == 4 {
            record[8..10].copy_from_slice(&1u16.to_le_bytes());
            record[10..12].copy_from_slice(&1u16.to_le_bytes());
            record[32] = 1;
            record[33] = 1;
            record[72] = 1;
        } else {
            if kind != 1 {
                record[30..32].copy_from_slice(&1u16.to_le_bytes());
            }
            record[32] = 1;
        }
        output.extend(record);
    }
    Some(output)
}
