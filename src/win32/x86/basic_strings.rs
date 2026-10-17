//! PE32 string helpers borrow only checked guest memory and typed arguments.
use crate::{
    api::Args,
    mem::GuestMem,
    win32::{catalog::BasicStringApi, text::units32 as units},
};
use std::cmp::Ordering;

pub(crate) fn call(id: BasicStringApi, a: &Args, mem: &mut GuestMem) -> Result<u32, u32> {
    use BasicStringApi::*;
    let wide = matches!(
        id,
        CompareOrdinal | CompareW | CompareInsensitiveW | LengthW | CopyW | ConcatW | CopyCountW
    );
    match id {
        Length | LengthA | LengthW | CopyA | CopyW | ConcatA | ConcatW | CopyCountA
        | CopyCountW => {
            let unit = if wide { 2 } else { 1 };
            let read = |p: u32, n: u32, truncated: bool| -> Result<Vec<u8>, u32> {
                let mut bytes = Vec::new();
                for i in 0..n.min(1 << 20) {
                    let address = p.checked_add(i * unit).ok_or(87u32)?;
                    let c = if wide {
                        mem.read_u16(address).map_err(|_| 87u32)?
                    } else {
                        mem.read_u8(address).map_err(|_| 87u32)? as u16
                    };
                    if c == 0 {
                        return Ok(bytes);
                    }
                    if wide {
                        bytes.extend(c.to_le_bytes());
                    } else {
                        bytes.push(c as u8);
                    }
                }
                if truncated && n <= 1 << 20 {
                    Ok(bytes)
                } else {
                    Err(87)
                }
            };
            if matches!(id, Length | LengthA | LengthW) {
                return if a[0] == 0 {
                    Ok(0)
                } else {
                    Ok(read(a[0], 1 << 20, false)?.len() as u32 / unit)
                };
            }
            let limited = matches!(id, CopyCountA | CopyCountW);
            if limited && (a[2] as i32) <= 0 {
                return Ok(a[0]);
            }
            let capacity = if limited { a[2] - 1 } else { 1 << 20 };
            let mut bytes = read(a[1], capacity, limited)?;
            bytes.extend(vec![0; unit as usize]);
            let offset = if matches!(id, ConcatA | ConcatW) {
                read(a[0], 1 << 20, false)?.len() as u32
            } else {
                0
            };
            mem.write_bytes(a[0].checked_add(offset).ok_or(87u32)?, &bytes)
                .map_err(|_| 87u32)?;
            Ok(a[0])
        }
        CompareOrdinal | CompareA | CompareW | CompareInsensitiveA | CompareInsensitiveW => {
            let ordinal = id == CompareOrdinal;
            let mut left = if ordinal {
                units(mem, a[0], a[1])?
            } else {
                crate::menu::read_text(mem, a[0], wide)?.unwrap_or_default()
            };
            let mut right = if ordinal {
                units(mem, a[2], a[3])?
            } else {
                crate::menu::read_text(mem, a[1], wide)?.unwrap_or_default()
            };
            if (ordinal && a[4] != 0) || matches!(id, CompareInsensitiveA | CompareInsensitiveW) {
                for text in [&mut left, &mut right] {
                    for c in text {
                        if let Some(ch) = char::from_u32(*c as u32) {
                            let mut uppercase = ch.to_uppercase();
                            let n = uppercase.next().unwrap();
                            if uppercase.next().is_none() && n as u32 <= 65535 {
                                *c = n as u16;
                            }
                        }
                    }
                }
            }
            let result = match left.cmp(&right) {
                Ordering::Less => -1i32,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            };
            Ok(if ordinal {
                (result + 2) as u32
            } else {
                result as u32
            })
        }
    }
}
