//! Calendar pictures use the shared locale vocabulary and process zone.
use super::*;
use crate::win32::{calendar::calendar_to_ticks, timezone};

pub(super) fn implemented(id: ApiId) -> bool {
    matches!(
        id,
        ApiId::Nls(
            N::GetDateFormatA
                | N::GetDateFormatW
                | N::GetDateFormatEx
                | N::GetTimeFormatA
                | N::GetTimeFormatW
                | N::GetTimeFormatEx
        )
    )
}

pub(super) fn call(
    id: N,
    a: &[u64; 16],
    memory: &mut Memory,
    sample: u64,
    zone: &timezone::Profile,
) -> Result<u64, Failure> {
    let date = matches!(
        id,
        N::GetDateFormatA | N::GetDateFormatW | N::GetDateFormatEx
    );
    let narrow = matches!(id, N::GetDateFormatA | N::GetTimeFormatA);
    let ex = matches!(id, N::GetDateFormatEx | N::GetTimeFormatEx);
    let capacity = a[5] as u32 as i32;
    if capacity < 0 || capacity != 0 && a[4] == 0 || id == N::GetDateFormatEx && a[6] != 0 {
        return Err(Failure::Windows(87));
    }
    let lcid = if ex {
        locale_id(memory, a[0])?
    } else {
        lcid(a[0] as u32)?
    };
    let flags = a[1] as u32;
    let common = 0xc0000000;
    let allowed = common | if date { 0xff } else { 0xf };
    if flags & !allowed != 0
        || date && (flags & 0x8b).count_ones() > 1
        || date && (flags & 0x30 == 0x30 || flags & 0x40 != 0 && flags & 0x30 != 0)
        || a[3] != 0 && (flags & 0x80000000 != 0 || date && flags & 0x8b != 0)
    {
        return Err(Failure::Windows(1004));
    }
    // The vocabulary has Gregorian dates; alternate eras and explicit bidi marks
    // require calendar/layout data that is not part of this profile.
    if date && flags & 0x34 != 0 {
        return Err(Failure::Windows(50));
    }
    let japanese = matches!(lcid, 0x411 | 0x11);
    let cp = ansi_codepage(lcid, flags & 0x40000000 != 0);
    let pattern = if a[3] == 0 {
        let pattern = if date {
            match (japanese, lcid == 0x7f, flags & 0x8b) {
                (true, _, 2) => "yyyy'年'M'月'd'日'",
                (true, _, 8) => "yyyy'年'M'月'",
                (true, _, 0x80) => "M'月'd'日'",
                (true, _, _) => "yyyy/MM/dd",
                (false, true, 2) => "dddd, dd MMMM yyyy",
                (false, true, 8) => "yyyy MMMM",
                (false, true, 0x80) => "MMMM dd",
                (false, true, _) => "MM/dd/yyyy",
                (false, false, 2) => "dddd, MMMM d, yyyy",
                (false, false, 8) => "MMMM yyyy",
                (false, false, 0x80) => "MMMM d",
                (false, false, _) => "M/d/yyyy",
            }
        } else if japanese {
            "H:mm:ss"
        } else if lcid == 0x7f {
            "HH:mm:ss"
        } else {
            "h:mm:ss tt"
        };
        pattern.encode_utf16().collect::<Vec<_>>()
    } else if narrow {
        let input = read(memory, a[3], u64::MAX, 1)?;
        crate::codepage::decode_lossy(&input[..input.len() - 1], cp)
            .map_err(Failure::Windows)?
            .encode_utf16()
            .collect()
    } else {
        let input = read(memory, a[3], u64::MAX, 2)?;
        input[..input.len() - 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect()
    };
    let mut fields = if a[2] == 0 {
        zone.local(sample).map_err(Failure::Windows)?
    } else {
        let mut bytes = [0; 16];
        memory.read(a[2], &mut bytes)?;
        timezone::fields(&bytes)
    };
    if date {
        let mut day = fields;
        day[4..].fill(0);
        let ticks = calendar_to_ticks(day)
            .filter(|_| fields[0] <= 9999)
            .ok_or(Failure::Windows(87))?;
        fields[2] = ((ticks / 864000000000 + 1) % 7) as u16;
    } else if fields[4] > 23 || fields[5] > 59 || fields[6] > 59 || fields[7] > 999 {
        return Err(Failure::Windows(87));
    }
    let mut output = picture(&pattern, fields, japanese, date, flags).map_err(Failure::Windows)?;
    output.push(0);
    let bytes: Vec<_> = output.into_iter().flat_map(u16::to_le_bytes).collect();
    let bytes = if narrow {
        crate::codepage::encode(&decode_utf16(&bytes, false).map_err(Failure::Windows)?, cp)
            .map_err(Failure::Windows)?
            .0
    } else {
        bytes
    };
    write(memory, a[4], a[5], &bytes, if narrow { 1 } else { 2 })
}

/// Preserve UTF-16 literals while interpreting only the calendar picture letters.
fn picture(
    pattern: &[u16],
    fields: [u16; 8],
    japanese: bool,
    date: bool,
    flags: u32,
) -> Result<Vec<u16>, u32> {
    let mut output = Vec::new();
    let (mut at, mut quoted, mut skip, mut skip_spaces, mut field_end) =
        (0, false, false, false, 0);
    while at < pattern.len() {
        let unit = pattern[at];
        if unit == b'\'' as u16 {
            if pattern.get(at + 1) == Some(&unit) {
                if !skip {
                    output.push(unit);
                }
                at += 2;
            } else {
                quoted = !quoted;
                at += 1;
            }
            continue;
        }
        let letter = char::from_u32(unit.into()).unwrap_or('\0');
        let field = if date { "yMdg" } else { "Hhmst" }.contains(letter);
        if quoted || !field {
            if !skip || !quoted && !skip_spaces && unit == b' ' as u16 {
                output.push(unit);
            }
            at += 1;
            continue;
        }
        let end = at + pattern[at..].iter().take_while(|&&c| c == unit).count();
        let suppressed = !date
            && match letter {
                'm' => flags & 1 != 0,
                's' => flags & 3 != 0,
                't' => flags & 4 != 0,
                _ => false,
            };
        if suppressed {
            output.truncate(field_end);
            skip = true;
            skip_spaces = letter == 't';
        } else {
            let letter = if !date && letter == 'h' && flags & 8 != 0 {
                'H'
            } else {
                letter
            };
            let count = if date {
                (end - at).min(4)
            } else {
                (end - at).min(2)
            };
            let token: String = std::iter::repeat_n(letter, count).collect();
            output.extend(locale::format(&token, fields, japanese, date)?.encode_utf16());
            field_end = output.len();
            skip = false;
        }
        at = end;
    }
    Ok(output)
}
