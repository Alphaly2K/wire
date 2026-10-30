//! Bounded NLS transfers over the same VMM as generated code and Windows calls.
mod calendar;
mod characters;
use super::{
    call::Call,
    catalog::{ApiId, NlsApi as N},
    context, locale,
    modules::{name, Failure},
    utf16::{decode_utf16, utf16_bytes},
    ServiceError,
};
use crate::{
    environment::GuestEnvironment,
    machine::{Mode, State},
    memory::{Fault, FaultKind, Memory, READ, WRITE},
};

pub(super) fn implemented(id: ApiId) -> bool {
    calendar::implemented(id)
        || characters::implemented(id)
        || matches!(
            id,
            ApiId::Nls(
                N::GetAcp
                    | N::GetOemcp
                    | N::GetConsoleCp
                    | N::GetConsoleOutputCp
                    | N::GetCpInfo
                    | N::GetCpInfoExA
                    | N::GetCpInfoExW
                    | N::IsValidCodePage
                    | N::IsDbcsLeadByte
                    | N::IsDbcsLeadByteEx
                    | N::MultiByteToWideChar
                    | N::WideCharToMultiByte
                    | N::GetUserDefaultLcid
                    | N::GetSystemDefaultLcid
                    | N::GetThreadLocale
                    | N::SetThreadLocale
                    | N::IsValidLocale
                    | N::EnumSystemLocalesA
                    | N::EnumSystemLocalesW
                    | N::EnumSystemLocalesEx
                    | N::GetUserDefaultLangId
                    | N::GetSystemDefaultLangId
                    | N::GetUserDefaultLocaleName
                    | N::GetSystemDefaultLocaleName
                    | N::GetLocaleInfoA
                    | N::GetLocaleInfoW
                    | N::GetLocaleInfoEx
                    | N::LocaleNameToLcid
                    | N::LcidToLocaleName
                    | N::IsValidLocaleName
                    | N::CompareStringA
                    | N::CompareStringW
                    | N::CompareStringEx
            )
        )
}
pub(super) fn clock_source(id: ApiId, state: &State, memory: &Memory) -> super::ClockSource {
    if calendar::implemented(id) && super::call::peek_slot(state, memory, 2) == Some(0) {
        super::ClockSource::Wall
    } else {
        super::ClockSource::None
    }
}
fn codepage(cp: u32, state: &State, memory: &Memory) -> Result<u32, Failure> {
    match cp {
        0 | 1 | 932 => Ok(932),
        3 => Ok(
            if matches!(
                memory.read_int(context::locale_address(state, memory.mode()), 4)? as u32,
                9 | 0x409
            ) {
                1252
            } else {
                932
            },
        ),
        1252 | 65001 => Ok(cp),
        _ => Err(Failure::Windows(87)),
    }
}
fn thread_locale(state: &State, memory: &mut Memory) -> Result<u32, Failure> {
    let address = context::locale_address(state, memory.mode());
    let value = memory.read_int(address, 4)? as u32;
    if value == 0 {
        memory.write_int(address, 4, 0x411)?;
        Ok(0x411)
    } else {
        Ok(value)
    }
}
fn read(memory: &Memory, pointer: u64, count: u64, width: usize) -> Result<Vec<u8>, Failure> {
    let count = count as u32 as i32;
    if pointer == 0 || count == 0 || count < -1 {
        return Err(Failure::Windows(87));
    }
    if count > 0 {
        let length = (count as usize)
            .checked_mul(width)
            .filter(|&n| n <= 16 * 1024 * 1024)
            .ok_or(Failure::Windows(8))?;
        let mut bytes = vec![0; length];
        memory.read(pointer, &mut bytes)?;
        return Ok(bytes);
    }
    let mut bytes = Vec::new();
    for n in 0..32768 {
        let at = pointer.checked_add((n * width) as u64).ok_or(Fault {
            address: pointer,
            length: width,
            access: READ,
            kind: FaultKind::Address,
        })?;
        let value = memory.read_int(at, width)?;
        bytes.extend_from_slice(&value.to_le_bytes()[..width]);
        if value == 0 {
            return Ok(bytes);
        }
    }
    Err(Failure::Windows(87))
}
fn write(
    memory: &mut Memory,
    pointer: u64,
    capacity: u64,
    bytes: &[u8],
    width: usize,
) -> Result<u64, Failure> {
    let capacity = capacity as u32 as i32;
    if capacity < 0 {
        return Err(Failure::Windows(87));
    }
    let required = bytes.len() / width;
    if capacity != 0 {
        if (capacity as usize) < required {
            return Err(Failure::Windows(122));
        }
        memory.write(pointer, bytes)?;
    }
    Ok(required as u64)
}
fn locale_id(memory: &Memory, pointer: u64) -> Result<u32, Failure> {
    if pointer == 0 || pointer == memory.mode().address(u64::MAX) {
        return Ok(0x411);
    }
    locale::id(&name(memory, pointer, true)?).ok_or(Failure::Windows(87))
}
fn lcid(id: u32) -> Result<u32, Failure> {
    if id == 0 {
        return Ok(0x411);
    }
    locale::name(id)
        .and_then(locale::id)
        .ok_or(Failure::Windows(87))
}
fn ansi_codepage(lcid: u32, use_acp: bool) -> u32 {
    if !use_acp && matches!(lcid, 9 | 0x409) {
        1252
    } else {
        932
    }
}
fn encode(
    text: &str,
    cp: u32,
    default: &[u8],
    no_best_fit: bool,
) -> Result<(Vec<u8>, bool), Failure> {
    if default == b"?" && !no_best_fit {
        return crate::codepage::encode(text, cp).map_err(Failure::Windows);
    }
    let mut result = Vec::new();
    let mut used = false;
    for ch in text.chars() {
        let mut buffer = [0; 4];
        let scalar: &str = ch.encode_utf8(&mut buffer);
        let (bytes, replaced) = crate::codepage::encode(scalar, cp).map_err(Failure::Windows)?;
        if replaced
            || no_best_fit && !crate::codepage::decode(&bytes, cp).is_ok_and(|s| s == scalar)
        {
            result.extend_from_slice(default);
            used = true;
        } else {
            result.extend_from_slice(&bytes);
        }
    }
    Ok((result, used))
}
fn convert(id: N, a: &[u64; 16], state: &State, memory: &mut Memory) -> Result<u64, Failure> {
    let narrow = id == N::MultiByteToWideChar;
    let cp = codepage(a[0] as u32, state, memory)?;
    let flags = a[1] as u32;
    if a[2] == a[4] && a[5] != 0 {
        return Err(Failure::Windows(87));
    }
    if (a[5] as u32 as i32) < 0 {
        return Err(Failure::Windows(87));
    }
    if narrow {
        if flags & !(if cp == 65001 { 8 } else { 9 }) != 0 {
            return Err(Failure::Windows(
                if cp != 65001 && flags & !15 == 0 && flags & 3 != 3 {
                    50
                } else {
                    1004
                },
            ));
        }
        let input = read(memory, a[2], a[3], 1)?;
        let text = if flags & 8 != 0 {
            crate::codepage::decode(&input, cp)
        } else {
            crate::codepage::decode_lossy(&input, cp)
        }
        .map_err(Failure::Windows)?;
        return write(memory, a[4], a[5], &utf16_bytes(&text), 2);
    }
    if cp == 65001 {
        if a[6] != 0 || a[7] != 0 {
            return Err(Failure::Windows(87));
        }
        if flags & !0x80 != 0 {
            return Err(Failure::Windows(1004));
        }
    } else if flags & !0x400 != 0 {
        return Err(Failure::Windows(if flags & !0x670 == 0 {
            50
        } else {
            1004
        }));
    }
    let default = if a[6] == 0 {
        vec![b'?']
    } else {
        let first = memory.read_int(a[6], 1)? as u8;
        if cp == 932 && matches!(first, 0x81..=0x9f | 0xe0..=0xfc) {
            let mut bytes = vec![0; 2];
            memory.read(a[6], &mut bytes)?;
            bytes
        } else {
            vec![first]
        }
    };
    let input = read(memory, a[2], a[3], 2)?;
    let text = decode_utf16(&input, flags & 0x80 != 0).map_err(Failure::Windows)?;
    let (output, used) = encode(&text, cp, &default, flags & 0x400 != 0)?;
    if a[5] != 0 && a[5] < output.len() as u64 {
        return Err(Failure::Windows(122));
    }
    if a[5] != 0 {
        memory.check(a[4], output.len(), WRITE)?;
    }
    if a[7] != 0 {
        memory.check(a[7], 4, WRITE)?;
    }
    if a[5] != 0 {
        memory.write(a[4], &output)?;
    }
    if a[7] != 0 {
        memory.write_int(a[7], 4, u64::from(used))?;
    }
    Ok(output.len() as u64)
}
fn call(
    id: N,
    a: &[u64; 16],
    state: &State,
    memory: &mut Memory,
    sample: u64,
    zone: &super::timezone::Profile,
) -> Result<u64, Failure> {
    if calendar::implemented(ApiId::Nls(id)) {
        return calendar::call(id, a, memory, sample, zone);
    }
    if characters::implemented(ApiId::Nls(id)) {
        return characters::call(id, a, memory);
    }
    let profile = if memory.mode() == Mode::X86 {
        GuestEnvironment::pe32()
    } else {
        GuestEnvironment::amd64()
    };
    Ok(match id {
        N::CompareStringA | N::CompareStringW | N::CompareStringEx => {
            if a[1] & !0x5803103f != 0 {
                return Err(Failure::Windows(1004));
            }
            if id == N::CompareStringEx {
                locale_id(memory, a[0])?;
            } else {
                lcid(a[0] as u32)?;
            }
            if a[2] == 0 || a[4] == 0 {
                return Err(Failure::Windows(87));
            }
            return Err(Failure::Windows(50));
        }
        N::GetAcp | N::GetOemcp | N::GetConsoleCp | N::GetConsoleOutputCp => {
            profile.codepage().into()
        }
        N::GetUserDefaultLcid
        | N::GetSystemDefaultLcid
        | N::GetUserDefaultLangId
        | N::GetSystemDefaultLangId => profile.locale_id().into(),
        N::GetThreadLocale => thread_locale(state, memory)?.into(),
        N::SetThreadLocale => {
            let lcid = match a[0] as u32 {
                0 | 0x400 | 0x800 => 0x411,
                9 => 0x409,
                0x11 => 0x411,
                n => n,
            };
            if locale::name(lcid).is_none() {
                return Err(Failure::Windows(87));
            }
            let at = context::locale_address(state, memory.mode());
            if thread_locale(state, memory)? != lcid {
                memory.write_int(at, 4, lcid.into())?;
            }
            1
        }
        N::IsValidLocale => u64::from(
            matches!(a[1] as u32, 0 | 1 | 2 | 0x39)
                && matches!(a[0] as u32, 9 | 0x11 | 0x409 | 0x411 | 0x7f),
        ),
        N::IsValidCodePage => u64::from(matches!(a[0] as u32, 932 | 1252 | 65001)),
        N::IsDbcsLeadByte | N::IsDbcsLeadByteEx => {
            let cp = if id == N::IsDbcsLeadByte {
                932
            } else {
                codepage(a[0] as u32, state, memory)?
            };
            let byte = a[usize::from(id == N::IsDbcsLeadByteEx)] as u8;
            u64::from(cp == 932 && matches!(byte, 0x81..=0x9f | 0xe0..=0xfc))
        }
        N::GetCpInfo | N::GetCpInfoExA | N::GetCpInfoExW => {
            let extended = id != N::GetCpInfo;
            if extended && a[1] != 0 {
                return Err(Failure::Windows(1004));
            }
            let cp = codepage(a[0] as u32, state, memory)?;
            let wide = id == N::GetCpInfoExW;
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
                let unicode: u16 = if cp == 932 {
                    0x30fb
                } else if cp == 65001 {
                    0xfffd
                } else {
                    0x3f
                };
                info[18..20].copy_from_slice(&unicode.to_le_bytes());
                info[20..24].copy_from_slice(&cp.to_le_bytes());
                let title = match cp {
                    932 => "932 (Japanese Shift-JIS)",
                    1252 => "1252 (Western European)",
                    _ => "65001 (UTF-8)",
                };
                let bytes = if wide {
                    utf16_bytes(title)
                } else {
                    title.as_bytes().to_vec()
                };
                info[24..24 + bytes.len()].copy_from_slice(&bytes);
            }
            memory.write(a[if extended { 2 } else { 1 }], &info)?;
            1
        }
        N::MultiByteToWideChar | N::WideCharToMultiByte => return convert(id, a, state, memory),
        N::IsValidLocaleName => u64::from(
            a[0] != 0
                && a[0] != memory.mode().address(u64::MAX)
                && locale::id(&name(memory, a[0], true)?).is_some(),
        ),
        N::LocaleNameToLcid => {
            if a[1] & !0x08000000 != 0 {
                return Err(Failure::Windows(1004));
            }
            let id = locale_id(memory, a[0])?;
            u64::from(match id {
                9 if a[1] == 0 => 0x409,
                0x11 if a[1] == 0 => 0x411,
                _ => id,
            })
        }
        N::LcidToLocaleName | N::GetUserDefaultLocaleName | N::GetSystemDefaultLocaleName => {
            let (text, pointer, capacity) = if id == N::LcidToLocaleName {
                if a[3] & !0x08000000 != 0 {
                    return Err(Failure::Windows(87));
                }
                let locale = match a[0] as u32 {
                    9 if a[3] == 0 => 0x409,
                    0x11 if a[3] == 0 => 0x411,
                    n => n,
                };
                (
                    locale::name(locale).ok_or(Failure::Windows(87))?,
                    a[1],
                    a[2],
                )
            } else {
                if a[0] == 0 || (a[1] as u32 as i32) <= 0 {
                    return Err(Failure::Windows(122));
                }
                ("ja-JP", a[0], a[1])
            };
            write(
                memory,
                pointer,
                capacity,
                &utf16_bytes(&(text.to_owned() + "\0")),
                2,
            )?
        }
        N::GetLocaleInfoA | N::GetLocaleInfoW | N::GetLocaleInfoEx => {
            let lcid = if id == N::GetLocaleInfoEx {
                locale_id(memory, a[0])?
            } else {
                lcid(a[0] as u32)?
            };
            if a[1] & !(0x80000000 | 0x20000000 | 0xffff) != 0 {
                return Err(Failure::Windows(1004));
            }
            let data = locale::info(lcid, a[1] as u32 & !0x80000000).map_err(Failure::Windows)?;
            let narrow = id == N::GetLocaleInfoA;
            let data = if narrow && a[1] & 0x20000000 == 0 {
                let cp = ansi_codepage(lcid, false);
                crate::codepage::encode(&decode_utf16(&data, true).map_err(Failure::Windows)?, cp)
                    .map_err(Failure::Windows)?
                    .0
            } else {
                data
            };
            write(memory, a[2], a[3], &data, if narrow { 1 } else { 2 })?
        }
        _ => unreachable!("unbound NLS service"),
    })
}
pub(super) fn dispatch(
    id: N,
    call_frame: &Call,
    state: &State,
    memory: &mut Memory,
    sample: u64,
    zone: &super::timezone::Profile,
) -> Result<u64, ServiceError> {
    if calendar::implemented(ApiId::Nls(id))
        && call_frame.arguments[2] == 0
        && zone.local(sample).is_err()
    {
        return Err(ServiceError::Unavailable(
            "wall clock is outside the Windows calendar range",
        ));
    }
    match call(id, &call_frame.arguments, state, memory, sample, zone) {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(ServiceError::Fault(fault)),
        Err(Failure::Windows(code)) => {
            memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
            Ok(0)
        }
    }
}
