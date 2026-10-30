//! Small, explicit guest NLS vocabulary shared by enumeration and locale queries.
pub(crate) fn id(name: &str) -> Option<u32> {
    match name.to_ascii_lowercase().as_str() {
        "ja-jp" => Some(0x411),
        "en-us" => Some(0x409),
        "" => Some(0x7f),
        "ja" => Some(0x11),
        "en" => Some(9),
        _ => None,
    }
}
pub(crate) fn name(id: u32) -> Option<&'static str> {
    match id {
        0x411 | 0x400 | 0x800 => Some("ja-JP"),
        0x409 => Some("en-US"),
        0x7f => Some(""),
        0x11 => Some("ja"),
        9 => Some("en"),
        _ => None,
    }
}

pub(crate) fn enumeration(flags: u32) -> impl Iterator<Item = (&'static str, u32)> {
    [
        ("en-US", 0x21),
        ("ja-JP", 0x21),
        ("en", 0x11),
        ("ja", 0x11),
        ("", 0x21),
    ]
    .into_iter()
    .filter(move |&(_, kind)| flags == 0 || flags & kind != 0)
}
pub(crate) fn info(id: u32, kind: u32) -> Result<Vec<u8>, u32> {
    let japanese = matches!(id, 0x411 | 0x11);
    let invariant = id == 0x7f;
    let number = match kind & 0xffff {
        1 | 9 => Some(id),
        0xb => Some(if japanese {
            932u32
        } else if invariant {
            0
        } else {
            437
        }),
        0x1004 => Some(if japanese {
            932u32
        } else if invariant {
            0
        } else {
            1252
        }),
        _ => None,
    };
    if kind & 0x20000000 != 0 {
        return number.map(|v| v.to_le_bytes().to_vec()).ok_or(1004);
    }
    let text = if let Some(n) = number {
        if matches!(kind & 0xffff, 1 | 9) {
            format!("{n:04X}")
        } else {
            n.to_string()
        }
    } else {
        match kind & 0xffff {
            0x5c => name(id).ok_or(87u32)?,
            0x59 => {
                if japanese {
                    "ja"
                } else if invariant {
                    "iv"
                } else {
                    "en"
                }
            }
            0x5a => {
                if japanese {
                    "JP"
                } else if invariant {
                    "IV"
                } else {
                    "US"
                }
            }
            0x1001 => {
                if japanese {
                    "Japanese"
                } else if invariant {
                    "Invariant Language"
                } else {
                    "English"
                }
            }
            0x1002 => {
                if japanese {
                    "Japan"
                } else if invariant {
                    "Invariant Country"
                } else {
                    "United States"
                }
            }
            0x72 => {
                if japanese {
                    "Japanese (Japan)"
                } else if invariant {
                    "Invariant Language (Invariant Country)"
                } else {
                    "English (United States)"
                }
            }
            0x73 => {
                if japanese {
                    "日本語 (日本)"
                } else if invariant {
                    "Invariant Language (Invariant Country)"
                } else {
                    "English (United States)"
                }
            }
            _ => return Err(50),
        }
        .to_owned()
    };
    Ok(text
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect())
}

pub(crate) fn format(
    pattern: &str,
    fields: [u16; 8],
    japanese: bool,
    date: bool,
) -> Result<String, u32> {
    let [year, month, dayofweek, day, hour, minute, second, _] = fields;
    let chars: Vec<_> = pattern.chars().collect();
    let mut out = String::new();
    let mut n = 0;
    let mut quoted = false;
    while n < chars.len() {
        let ch = chars[n];
        if ch == '\'' {
            if chars.get(n + 1) == Some(&'\'') {
                out.push('\'');
                n += 2;
            } else {
                quoted = !quoted;
                n += 1;
            }
            continue;
        }
        if quoted {
            out.push(ch);
            n += 1;
            continue;
        }
        let mut end = n + 1;
        while chars.get(end) == Some(&ch) {
            end += 1;
        }
        let count = end - n;
        let numeric = |value: u16| {
            if count == 1 {
                value.to_string()
            } else {
                format!("{value:02}")
            }
        };
        let text = match ch {
            'y' if date && count <= 2 => numeric(year % 100),
            'y' if date => format!("{year:04}"),
            'M' if date && count <= 2 => numeric(month),
            'M' if date && japanese => format!("{month}月"),
            'M' if date => {
                let word = [
                    "January",
                    "February",
                    "March",
                    "April",
                    "May",
                    "June",
                    "July",
                    "August",
                    "September",
                    "October",
                    "November",
                    "December",
                ]
                .get(usize::from(month).wrapping_sub(1))
                .ok_or(87u32)?;
                if count == 3 {
                    word[..3].to_owned()
                } else {
                    (*word).to_owned()
                }
            }
            'd' if date && count <= 2 => numeric(day),
            'd' if date && japanese => {
                let word = ["日", "月", "火", "水", "木", "金", "土"]
                    .get(dayofweek as usize)
                    .ok_or(87u32)?;
                if count == 3 {
                    (*word).to_owned()
                } else {
                    format!("{word}曜日")
                }
            }
            'd' if date => {
                let word = [
                    "Sunday",
                    "Monday",
                    "Tuesday",
                    "Wednesday",
                    "Thursday",
                    "Friday",
                    "Saturday",
                ]
                .get(dayofweek as usize)
                .ok_or(87u32)?;
                if count == 3 {
                    word[..3].to_owned()
                } else {
                    (*word).to_owned()
                }
            }
            'H' if !date => numeric(hour),
            'h' if !date => numeric(if hour % 12 == 0 { 12 } else { hour % 12 }),
            'm' if !date => numeric(minute),
            's' if !date => numeric(second),
            't' if !date => {
                let marker = if japanese {
                    if hour < 12 {
                        "午前"
                    } else {
                        "午後"
                    }
                } else if hour < 12 {
                    "AM"
                } else {
                    "PM"
                };
                if count == 1 {
                    marker.chars().take(1).collect()
                } else {
                    marker.to_owned()
                }
            }
            'g' if date => return Err(50),
            _ => chars[n..end].iter().collect(),
        };
        out.push_str(&text);
        n = end;
    }
    Ok(out)
}

/// C1 flags needed by the CRT's character tables. Unicode properties follow
/// Rust's Unicode tables; this is not a complete Windows NLS database.
pub(crate) fn ctype1(unit: u16) -> u16 {
    let Some(c) = char::from_u32(unit as u32) else {
        return 0;
    };
    if (0xfdd0..=0xfdef).contains(&unit) || unit >= 0xfffe {
        return 0;
    }
    let mut flags = 0x200;
    if c.is_uppercase() {
        flags |= 1;
    }
    if c.is_lowercase() {
        flags |= 2;
    }
    if c.is_ascii_digit() || ('０'..='９').contains(&c) {
        flags |= 4;
    }
    if c.is_whitespace() {
        flags |= 8;
    }
    if c.is_control() {
        flags |= 0x20;
    }
    if matches!(
        c,
        '\t' | ' ' | '\u{a0}' | '\u{1680}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    ) || ('\u{2000}'..='\u{200a}').contains(&c)
    {
        flags |= 0x40;
    }
    if c.is_ascii_hexdigit()
        || ('Ａ'..='Ｆ').contains(&c)
        || ('ａ'..='ｆ').contains(&c)
        || ('０'..='９').contains(&c)
    {
        flags |= 0x80;
    }
    if c.is_alphabetic() {
        flags |= 0x100;
    }
    if !c.is_alphanumeric() && !c.is_whitespace() && !c.is_control() {
        flags |= 0x10;
    }
    flags
}
