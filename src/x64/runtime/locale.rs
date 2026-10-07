//! Small, explicit guest NLS vocabulary shared by enumeration and locale queries.
pub(super) fn id(name: &str) -> Option<u32> {
    match name.to_ascii_lowercase().as_str() {
        "ja-jp" => Some(0x411),
        "en-us" => Some(0x409),
        "" => Some(0x7f),
        "ja" => Some(0x11),
        "en" => Some(9),
        _ => None,
    }
}
pub(super) fn name(id: u32) -> Option<&'static str> {
    match id {
        0x411 | 0x400 | 0x800 => Some("ja-JP"),
        0x409 => Some("en-US"),
        0x7f => Some(""),
        0x11 => Some("ja"),
        9 => Some("en"),
        _ => None,
    }
}
pub(super) fn info(id: u32, kind: u32) -> Result<Vec<u8>, u32> {
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

pub(super) fn format(
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
                .get(month as usize - 1)
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

impl super::Runtime64 {
    pub(super) fn format_calendar(
        &self,
        mem: &mut super::Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        let date = name == "GetDateFormatEx";
        let locale = if args[0] == 0 {
            Some(0x411)
        } else {
            id(&Self::utf16(mem, args[0])?)
        };
        let flags = args[1] as u32;
        let custom = if args[3] == 0 {
            None
        } else {
            Some(Self::utf16(mem, args[3])?)
        };
        let mut fields = [0u16; 8];
        if args[2] == 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?;
            let instant = now.as_secs() as libc::time_t;
            let mut calendar: libc::tm = unsafe { std::mem::zeroed() };
            if unsafe { libc::localtime_r(&instant, &mut calendar) }.is_null() {
                return Err("host local calendar unavailable".into());
            }
            fields = [
                (calendar.tm_year + 1900) as u16,
                (calendar.tm_mon + 1) as u16,
                calendar.tm_wday as u16,
                calendar.tm_mday as u16,
                calendar.tm_hour as u16,
                calendar.tm_min as u16,
                calendar.tm_sec as u16,
                now.subsec_millis() as u16,
            ];
        } else {
            mem.check(args[2], 16, super::READ)?;
            for (n, value) in fields.iter_mut().enumerate() {
                *value = mem.read_int(args[2] + n as u64 * 2, 2)? as u16;
            }
        }
        let output = (|| -> Result<Vec<u8>, u32> {
            let id = locale.ok_or(87u32)?;
            if id == 0x7f {
                return Err(50);
            }
            if custom.is_some() && flags != 0 || (args[5] as i32) < 0 {
                return Err(1004);
            }
            let japanese = matches!(id, 0x411 | 0x11);
            let pattern = if date {
                if args[6] != 0 || flags & !0x8000000b != 0 || (flags & 0xb).count_ones() > 1 {
                    return Err(1004);
                }
                let mut day_fields = fields;
                day_fields[4..].fill(0);
                let ticks = Self::calendar_to_ticks(day_fields).ok_or(87u32)?;
                if fields[0] > 9999 {
                    return Err(87);
                }
                fields[2] = ((ticks / 864000000000 + 1) % 7) as u16;
                custom.unwrap_or_else(|| match (japanese, flags & 0xb) {
                    (true, 2) => "yyyy'年'M'月'd'日'".into(),
                    (false, 2) => "dddd, MMMM d, yyyy".into(),
                    (true, 8) => "yyyy'年'M'月'".into(),
                    (false, 8) => "MMMM yyyy".into(),
                    (true, _) => "yyyy/MM/dd".into(),
                    (false, _) => "M/d/yyyy".into(),
                })
            } else {
                if flags & !0xf != 0 || fields[4] > 23 || fields[5] > 59 || fields[6] > 59 {
                    return Err(87);
                }
                custom.unwrap_or_else(|| {
                    let mut p = if japanese || flags & 8 != 0 {
                        "H".to_owned()
                    } else {
                        "h".to_owned()
                    };
                    if flags & 1 == 0 {
                        p.push_str(":mm");
                        if flags & 2 == 0 {
                            p.push_str(":ss");
                        }
                    }
                    if !japanese && flags & 12 == 0 {
                        p.push_str(" tt");
                    }
                    p
                })
            };
            Ok(format(&pattern, fields, japanese, date)?
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect())
        })();
        match output {
            Err(error) => {
                self.set_error(mem, error)?;
                Ok(0)
            }
            Ok(bytes) => {
                let size = bytes.len() as u64 / 2;
                if args[5] != 0 && args[5] < size {
                    self.set_error(mem, 122)?;
                    Ok(0)
                } else {
                    if args[5] != 0 {
                        mem.write(args[4], &bytes)?;
                    }
                    Ok(size)
                }
            }
        }
    }
}
