//! Stateless Windows code pages in ordinary WASM memory. No host conversion.
//! Frozen mappings preserve the existing iconv reference, including duplicate
//! CP932 encodings and nonreversible CP1252 expansions (not just round trips).
const INVALID: u32 = u32::MAX;
const SINGLE932: &[u8] = include_bytes!("cp932-single.bin");
const DOUBLE932: &[u8] = include_bytes!("cp932-double.bin");
const ENCODE932: &[u8] = include_bytes!("cp932-encode.bin");
const PAYLOAD932: &[u8] = include_bytes!("cp932-payload.bin");
const SINGLE1252: &[u8] = include_bytes!("cp1252-single.bin");
const ENCODE1252: &[u8] = include_bytes!("cp1252-encode.bin");
const PAYLOAD1252: &[u8] = include_bytes!("cp1252-payload.bin");

fn codepage(cp: u32) -> Result<u32, u32> {
    match cp {
        0 | 1 | 3 | 932 => Ok(932),
        1252 | 65001 => Ok(cp),
        _ => Err(87),
    }
}
fn dword(table: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(table[offset..offset + 4].try_into().unwrap())
}
fn lead(byte: u8) -> Option<usize> {
    match byte {
        0x81..=0x9f => Some((byte - 0x81) as usize),
        0xe0..=0xfc => Some((byte - 0xe0) as usize + 31),
        _ => None,
    }
}
pub fn decode(input: &[u8], cp: u32) -> Result<String, u32> {
    let cp = codepage(cp)?;
    if cp == 65001 || input.is_ascii() {
        return String::from_utf8(input.to_vec()).map_err(|_| 1113);
    }
    let single = if cp == 932 { SINGLE932 } else { SINGLE1252 };
    let mut output = String::new();
    let mut at = 0;
    while at < input.len() {
        let mut scalar = dword(single, input[at] as usize * 4);
        if scalar == INVALID && cp == 932 {
            let page = lead(input[at]).ok_or(1113u32)?;
            let trail = *input.get(at + 1).ok_or(1113u32)?;
            scalar = dword(DOUBLE932, (page * 256 + trail as usize) * 4);
            at += 1;
        }
        output.push(char::from_u32(scalar).ok_or(1113u32)?);
        at += 1;
    }
    Ok(output)
}
pub fn decode_lossy(input: &[u8], cp: u32) -> Result<String, u32> {
    if let Ok(text) = decode(input, cp) {
        return Ok(text);
    }
    let cp = codepage(cp)?;
    if cp == 65001 {
        return Ok(String::from_utf8_lossy(input).into_owned());
    }
    let mut output = String::new();
    let mut at = 0;
    while at < input.len() {
        let width = if cp == 932 && lead(input[at]).is_some() && at + 1 < input.len() {
            2
        } else {
            1
        };
        match decode(&input[at..at + width], cp) {
            Ok(text) => {
                output.push_str(&text);
                at += width;
            }
            Err(_) => {
                output.push('\u{fffd}');
                at += 1;
            }
        }
    }
    Ok(output)
}
fn encoded(table: &[u8], payload: &'static [u8], scalar: u32) -> Option<&'static [u8]> {
    let (mut low, mut high) = (0, table.len() / 9);
    while low < high {
        let mid = low + (high - low) / 2;
        let at = mid * 9;
        match dword(table, at).cmp(&scalar) {
            std::cmp::Ordering::Less => low = mid + 1,
            std::cmp::Ordering::Greater => high = mid,
            std::cmp::Ordering::Equal => {
                let start = dword(table, at + 4) as usize;
                return Some(&payload[start..start + table[at + 8] as usize]);
            }
        }
    }
    None
}
pub fn encode(input: &str, cp: u32) -> Result<(Vec<u8>, bool), u32> {
    let cp = codepage(cp)?;
    if cp == 65001 || input.is_ascii() {
        return Ok((input.as_bytes().to_vec(), false));
    }
    let (table, payload) = if cp == 932 {
        (ENCODE932, PAYLOAD932)
    } else {
        (ENCODE1252, PAYLOAD1252)
    };
    let mut output = Vec::new();
    let mut used_default = false;
    for scalar in input.chars() {
        if scalar.is_ascii() {
            output.push(scalar as u8);
        } else if let Some(bytes) = encoded(table, payload, scalar as u32) {
            output.extend_from_slice(bytes);
        } else {
            output.push(b'?');
            used_default = true;
        }
    }
    Ok((output, used_default))
}
