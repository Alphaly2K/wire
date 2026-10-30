//! UTF-16 byte conversion, independent of guest memory and execution.
pub(crate) fn utf16_bytes(text: &str) -> Vec<u8> {
    let units = if text.is_ascii() {
        text.len()
    } else {
        text.encode_utf16().count()
    };
    let mut output = vec![0; units * 2];
    for (bytes, unit) in output
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .zip(text.encode_utf16())
    {
        bytes.copy_from_slice(&unit.to_le_bytes());
    }
    output
}

pub(crate) fn decode_utf16(bytes: &[u8], strict: bool) -> Result<String, u32> {
    let units = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]));
    let mut text = String::with_capacity(bytes.len());
    for scalar in char::decode_utf16(units) {
        text.push(match scalar {
            Ok(ch) => ch,
            Err(_) if strict => return Err(1113),
            Err(_) => char::REPLACEMENT_CHARACTER,
        });
    }
    Ok(text)
}
