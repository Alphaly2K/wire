//! Windows GUID byte layout and canonical text; no guest memory or runtime state.
pub(crate) fn format_text(guid: [u8; 16]) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        u32::from_le_bytes(guid[..4].try_into().unwrap()),
        u16::from_le_bytes(guid[4..6].try_into().unwrap()),
        u16::from_le_bytes(guid[6..8].try_into().unwrap()),
        guid[8],
        guid[9],
        guid[10],
        guid[11],
        guid[12],
        guid[13],
        guid[14],
        guid[15]
    )
}

pub(crate) fn utf16_bytes(guid: [u8; 16]) -> Vec<u8> {
    format_text(guid)
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

pub(crate) fn parse(units: &[u16]) -> Option<[u8; 16]> {
    if units.len() != 38
        || units[0] != b'{' as u16
        || units[37] != b'}' as u16
        || [9, 14, 19, 24].iter().any(|&n| units[n] != b'-' as u16)
    {
        return None;
    }
    let mut bytes = [0; 16];
    for (n, index) in [1, 3, 5, 7, 10, 12, 15, 17, 20, 22, 25, 27, 29, 31, 33, 35]
        .into_iter()
        .enumerate()
    {
        let hex = |u| char::from_u32(u as u32)?.to_digit(16).map(|v| v as u8);
        bytes[n] = hex(units[index])? * 16 + hex(units[index + 1])?;
    }
    bytes[..4].reverse();
    bytes[4..6].reverse();
    bytes[6..8].reverse();
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    const GUID: [u8; 16] = [
        0x33, 0x22, 0x11, 0, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
    ];
    const TEXT: &str = "{00112233-4455-6677-8899-AABBCCDDEEFF}";
    #[test]
    fn canonical_layout_and_lowercase_input() {
        assert_eq!(format_text(GUID), TEXT);
        assert_eq!(
            utf16_bytes(GUID),
            TEXT.encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        );
        for text in [TEXT.to_owned(), TEXT.to_ascii_lowercase()] {
            assert_eq!(parse(&text.encode_utf16().collect::<Vec<_>>()), Some(GUID));
        }
    }
    #[test]
    fn canonical_parser_rejects_non_ascii_bad_separators_and_extra_storage() {
        let valid: Vec<_> = TEXT.encode_utf16().collect();
        for (index, unit) in [
            (0, b'(' as u16),
            (9, b'_' as u16),
            (3, 0xd800),
            (7, 0xff11),
            (37, b' ' as u16),
        ] {
            let mut invalid = valid.clone();
            invalid[index] = unit;
            assert_eq!(parse(&invalid), None);
        }
        assert_eq!(parse(&valid[..37]), None);
        let mut nul = valid;
        nul.push(0);
        assert_eq!(parse(&nul), None); // adapters remove their terminator explicitly
    }
}
