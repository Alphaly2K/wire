//! Checked Win64 NLS: layout/error policy stays separate from pure vocabulary.
use crate::{
    runtime::environment::GuestEnvironment,
    win32::{catalog::NlsApi, kernel::nls as locale},
    x64::memory::{Memory, WRITE},
};
pub(crate) fn call(
    mem: &mut Memory,
    id: NlsApi,
    args: &[u64],
    environment: &GuestEnvironment,
    error_slot: u64,
) -> Result<u64, String> {
    Ok(match id {
        NlsApi::GetDateFormatEx | NlsApi::GetTimeFormatEx => {
            format_calendar(mem, id == NlsApi::GetDateFormatEx, args, error_slot)?
        }
        NlsApi::GetUserDefaultLcid
        | NlsApi::GetSystemDefaultLcid
        | NlsApi::GetThreadLocale
        | NlsApi::GetUserDefaultLangId
        | NlsApi::GetSystemDefaultLangId => environment.locale_id() as u64,
        NlsApi::IsValidLocaleName => {
            (args[0] == 0 || locale::id(&super::text::utf16(mem, args[0])?).is_some()) as u64
        }
        NlsApi::LocaleNameToLcid => {
            if args[1] & !0x08000000 != 0 {
                mem.write_int(error_slot, 4, 1004)?;
                0
            } else {
                let id = if args[0] == 0 {
                    Some(0x411)
                } else {
                    locale::id(&super::text::utf16(mem, args[0])?)
                };
                match id {
                    Some(9) if args[1] == 0 => 0x409,
                    Some(0x11) if args[1] == 0 => 0x411,
                    Some(n) => n as u64,
                    None => {
                        mem.write_int(error_slot, 4, 87)?;
                        0
                    }
                }
            }
        }
        NlsApi::LcidToLocaleName
        | NlsApi::GetUserDefaultLocaleName
        | NlsApi::GetSystemDefaultLocaleName => {
            let (name, pointer, capacity) = if id == NlsApi::LcidToLocaleName {
                (
                    if args[3] & !0x08000000 == 0 {
                        locale::name(args[0] as u32)
                    } else {
                        None
                    },
                    args[1],
                    args[2],
                )
            } else {
                (Some("ja-JP"), args[0], args[1])
            };
            if let Some(name) = name {
                let data: Vec<_> = name
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect();
                if capacity != 0 && (capacity as i32 <= 0 || capacity < data.len() as u64 / 2) {
                    mem.write_int(error_slot, 4, 122)?;
                    0
                } else {
                    if capacity != 0 {
                        mem.write(pointer, &data)?;
                    }
                    data.len() as u64 / 2
                }
            } else {
                mem.write_int(error_slot, 4, 87)?;
                0
            }
        }
        NlsApi::GetLocaleInfoEx | NlsApi::GetLocaleInfoW => {
            let id = if id == NlsApi::GetLocaleInfoW {
                locale::name(args[0] as u32).and_then(locale::id)
            } else if args[0] == 0 {
                Some(0x411)
            } else {
                locale::id(&super::text::utf16(mem, args[0])?)
            };
            let data = if args[1] & !(0x80000000 | 0x20000000 | 0xffff) != 0 {
                Err(1004)
            } else {
                id.ok_or(87)
                    .and_then(|id| locale::info(id, args[1] as u32 & !0x80000000))
            };
            match data {
                Err(error) => {
                    mem.write_int(error_slot, 4, (error) as u64)?;
                    0
                }
                Ok(data) => {
                    let required = data.len() as u64 / 2;
                    if args[3] != 0 && (args[3] as i32 <= 0 || args[3] < required) {
                        mem.write_int(error_slot, 4, 122)?;
                        0
                    } else {
                        if args[3] != 0 {
                            mem.write(args[2], &data)?;
                        }
                        required
                    }
                }
            }
        }
        NlsApi::MultiByteToWideChar
        | NlsApi::WideCharToMultiByte
        | NlsApi::LcMapStringW
        | NlsApi::LcMapStringEx => {
            if id == NlsApi::LcMapStringEx {
                let locale = if args[0] == 0 {
                    "ja-JP".into()
                } else {
                    super::text::utf16(mem, args[0])?
                };
                if !matches!(locale.as_str(), "" | "ja-JP" | "en-US")
                    || args[6..].iter().any(|&a| a != 0)
                {
                    return Err("Win64 locale/version/sort handle not implemented".into());
                }
            }
            let wide_input = id != NlsApi::MultiByteToWideChar;
            let input =
                super::text::string_bytes(mem, args[2], args[3], if wide_input { 2 } else { 1 })?;
            let output: Result<(Vec<u8>, bool), u32> = if id == NlsApi::MultiByteToWideChar {
                if args[1] as u32 & !9 != 0 {
                    Err(1004)
                } else {
                    let text = if args[1] & 8 != 0 {
                        crate::codepage::decode(&input, args[0] as u32)
                    } else {
                        crate::codepage::decode_lossy(&input, args[0] as u32)
                    };
                    text.map(|text| {
                        (
                            text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
                            false,
                        )
                    })
                }
            } else {
                let units: Vec<u16> = input
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect();
                if id == NlsApi::LcMapStringW || id == NlsApi::LcMapStringEx {
                    let upper = args[1] & 0x200 != 0;
                    let lower = args[1] & 0x100 != 0;
                    if upper == lower || args[1] & !0x01000300 != 0 {
                        Err(1004)
                    } else {
                        String::from_utf16(&units).map_err(|_| 1113).map(|text| {
                            let mapped = if upper {
                                text.to_uppercase()
                            } else {
                                text.to_lowercase()
                            };
                            (
                                mapped.encode_utf16().flat_map(u16::to_le_bytes).collect(),
                                false,
                            )
                        })
                    }
                } else if args[6] != 0 {
                    return Err("Win64 custom codepage default character is not implemented".into());
                } else if args[1] & !0x480 != 0
                    || (args[0] as u32 == 65001 && (args[7] != 0 || args[1] & !0x80 != 0))
                {
                    Err(1004)
                } else {
                    let text = if args[1] & 0x80 != 0 {
                        String::from_utf16(&units).map_err(|_| 1113)
                    } else {
                        Ok(String::from_utf16_lossy(&units))
                    };
                    text.and_then(|text| crate::codepage::encode(&text, args[0] as u32))
                }
            };
            match output {
                Err(error) => {
                    mem.write_int(error_slot, 4, (error) as u64)?;
                    0
                }
                Ok((output, used)) => {
                    let count = output.len()
                        / if id == NlsApi::WideCharToMultiByte {
                            1
                        } else {
                            2
                        };
                    let capacity = args[5] as u32 as i32;
                    if capacity < 0 || (capacity != 0 && (capacity as usize) < count) {
                        mem.write_int(error_slot, 4, 122)?;
                        0
                    } else {
                        if capacity != 0 {
                            mem.check(args[4], output.len(), WRITE)?;
                        }
                        if id == NlsApi::WideCharToMultiByte && args[7] != 0 {
                            mem.check(args[7], 4, WRITE)?;
                        }
                        if capacity != 0 {
                            mem.write(args[4], &output)?;
                        }
                        if id == NlsApi::WideCharToMultiByte && args[7] != 0 {
                            mem.write_int(args[7], 4, used as u64)?;
                        }
                        count as u64
                    }
                }
            }
        }
        NlsApi::GetAcp | NlsApi::GetOemcp | NlsApi::GetConsoleOutputCp | NlsApi::GetConsoleCp => {
            environment.codepage() as u64
        }
        NlsApi::IsValidCodePage => matches!(args[0] as u32, 932 | 1252 | 65001) as u64,
        NlsApi::GetCpInfo | NlsApi::GetCpInfoExA | NlsApi::GetCpInfoExW => {
            let extended = id != NlsApi::GetCpInfo;
            let cp = match args[0] as u32 {
                0 | 1 | 3 => 932,
                value => value,
            };
            if extended && args[1] != 0 {
                mem.write_int(error_slot, 4, 1004)?;
                0
            } else if !matches!(cp, 932 | 1252 | 65001) {
                mem.write_int(error_slot, 4, 87)?;
                0
            } else {
                let wide = id == NlsApi::GetCpInfoExW;
                let mut info = vec![
                    0;
                    if !extended {
                        20
                    } else if wide {
                        544
                    } else {
                        284
                    }
                ];
                info[..4].copy_from_slice(
                    &(if cp == 932 {
                        2u32
                    } else if cp == 65001 {
                        4
                    } else {
                        1
                    })
                    .to_le_bytes(),
                );
                info[4] = b'?';
                if cp == 932 {
                    info[6..10].copy_from_slice(&[0x81, 0x9f, 0xe0, 0xfc]);
                }
                if extended {
                    let unicode_default = if cp == 932 {
                        0x30fbu16
                    } else if cp == 65001 {
                        0xfffd
                    } else {
                        0x3f
                    };
                    info[18..20].copy_from_slice(&unicode_default.to_le_bytes());
                    info[20..24].copy_from_slice(&cp.to_le_bytes());
                    let name = match cp {
                        932 => "932 (Japanese Shift-JIS)",
                        1252 => "1252 (Western European)",
                        _ => "65001 (UTF-8)",
                    };
                    let bytes: Vec<u8> = if wide {
                        name.encode_utf16().flat_map(u16::to_le_bytes).collect()
                    } else {
                        name.as_bytes().to_vec()
                    };
                    info[24..24 + bytes.len()].copy_from_slice(&bytes);
                }
                mem.write(args[if extended { 2 } else { 1 }], &info)?;
                1
            }
        }
        NlsApi::GetStringTypeW => {
            if args[0] as u32 != 1 {
                mem.write_int(error_slot, 4, 1004)?;
                0
            } else {
                let bytes = super::text::string_bytes(mem, args[1], args[2], 2)?;
                let output: Vec<u8> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .flat_map(|b| locale::ctype1(u16::from_le_bytes([b[0], b[1]])).to_le_bytes())
                    .collect();
                mem.write(args[3], &output)?;
                1
            }
        }

        _ => unreachable!("NLS API uses an absent or dedicated callback route"),
    })
}

fn format_calendar(
    mem: &mut Memory,
    date: bool,
    args: &[u64],
    error_slot: u64,
) -> Result<u64, String> {
    let locale = if args[0] == 0 {
        Some(0x411)
    } else {
        locale::id(&super::text::utf16(mem, args[0])?)
    };
    let flags = args[1] as u32;
    let custom = if args[3] == 0 {
        None
    } else {
        Some(super::text::utf16(mem, args[3])?)
    };
    let mut fields = [0u16; 8];
    if args[2] == 0 {
        fields = crate::host::calendar::local_system_time()?;
    } else {
        mem.check(args[2], 16, crate::x64::memory::READ)?;
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
            let ticks =
                crate::win32::kernel::calendar::calendar_to_ticks(day_fields).ok_or(87u32)?;
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
        Ok(locale::format(&pattern, fields, japanese, date)?
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect())
    })();
    match output {
        Err(error) => {
            mem.write_int(error_slot, 4, error as u64)?;
            Ok(0)
        }
        Ok(bytes) => {
            let size = bytes.len() as u64 / 2;
            if args[5] != 0 && args[5] < size {
                mem.write_int(error_slot, 4, 122)?;
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

/// Fill owned callback inputs after the process validates the callback frame and allocates its block.
pub(crate) fn enumeration_calls(
    mem: &mut Memory,
    block: u64,
    args: &[u64],
) -> Result<Vec<(u64, [u64; 4])>, String> {
    let mut calls = Vec::new();
    let mut at = block;
    for (locale, flags) in [("en-US", 0x21), ("ja-JP", 0x21), ("en", 0x11), ("ja", 0x11)] {
        let requested = args[1] as u32;
        if requested != 0 && requested & 7 != 0 && requested & 1 == 0 {
            continue;
        }
        if requested & 0x30 != 0 && requested & flags & 0x30 == 0 {
            continue;
        }
        let data: Vec<_> = locale
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        mem.write(at, &data)?;
        calls.push((args[0], [at, flags as u64, args[2], 0]));
        at += data.len() as u64;
    }
    Ok(calls)
}
