//! Bounded Windows string operations; memory and fault policy belong to adapters.
use super::catalog::BasicStringApi;
use std::cmp::Ordering;

pub(crate) trait Memory {
    type Error;
    fn add(&self, pointer: u64, offset: u64) -> Result<u64, Self::Error>;
    fn read(&self, pointer: u64, output: &mut [u8]) -> Result<(), Self::Error>;
    fn write(&mut self, pointer: u64, bytes: &[u8]) -> Result<(), Self::Error>;
    fn invalid(&self) -> Self::Error;
}
fn terminated<M: Memory>(
    mem: &M,
    pointer: u64,
    unit: usize,
    count: u32,
    truncated: bool,
) -> Result<Vec<u8>, M::Error> {
    let mut bytes = Vec::new();
    for i in 0..count.min(1 << 20) {
        let address = mem.add(pointer, u64::from(i) * unit as u64)?;
        let mut value = [0; 2];
        mem.read(address, &mut value[..unit])?;
        if value == [0; 2] {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&value[..unit]);
    }
    if truncated && count <= 1 << 20 {
        Ok(bytes)
    } else {
        Err(mem.invalid())
    }
}
fn text<M: Memory>(mem: &M, pointer: u64, wide: bool) -> Result<Vec<u16>, M::Error> {
    if pointer == 0 {
        return Ok(Vec::new());
    }
    let bytes = terminated(mem, pointer, if wide { 2 } else { 1 }, 32768, false)?;
    if wide {
        Ok(bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect())
    } else {
        crate::codepage::decode_lossy(&bytes, 932)
            .map(|text| text.encode_utf16().collect())
            .map_err(|_| mem.invalid())
    }
}
fn units<M: Memory>(mem: &M, pointer: u64, count: u32) -> Result<Vec<u16>, M::Error> {
    if pointer == 0 {
        return Err(mem.invalid());
    }
    if count == u32::MAX {
        return text(mem, pointer, true);
    }
    if count > 1 << 20 {
        return Err(mem.invalid());
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut bytes = vec![0; count as usize * 2];
    mem.read(pointer, &mut bytes)?;
    Ok(bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect())
}
pub(crate) fn call<M: Memory>(id: BasicStringApi, a: &[u64], mem: &mut M) -> Result<u64, M::Error> {
    use BasicStringApi::*;
    let wide = matches!(
        id,
        CompareOrdinal | CompareW | CompareInsensitiveW | LengthW | CopyW | ConcatW | CopyCountW
    );
    match id {
        Length | LengthA | LengthW | CopyA | CopyW | ConcatA | ConcatW | CopyCountA
        | CopyCountW => {
            let unit = if wide { 2 } else { 1 };
            if matches!(id, Length | LengthA | LengthW) {
                return if a[0] == 0 {
                    Ok(0)
                } else {
                    Ok((terminated(mem, a[0], unit, 1 << 20, false)?.len() / unit) as u64)
                };
            }
            let limited = matches!(id, CopyCountA | CopyCountW);
            if limited && a[2] as u32 as i32 <= 0 {
                return Ok(a[0]);
            }
            let capacity = if limited { a[2] as u32 - 1 } else { 1 << 20 };
            let mut bytes = terminated(mem, a[1], unit, capacity, limited)?;
            bytes.resize(bytes.len() + unit, 0);
            let offset = if matches!(id, ConcatA | ConcatW) {
                terminated(mem, a[0], unit, 1 << 20, false)?.len() as u64
            } else {
                0
            };
            let destination = mem.add(a[0], offset)?;
            mem.write(destination, &bytes)?;
            Ok(a[0])
        }
        CompareOrdinal | CompareA | CompareW | CompareInsensitiveA | CompareInsensitiveW => {
            let ordinal = id == CompareOrdinal;
            let mut left = if ordinal {
                units(mem, a[0], a[1] as u32)?
            } else {
                text(mem, a[0], wide)?
            };
            let mut right = if ordinal {
                units(mem, a[2], a[3] as u32)?
            } else {
                text(mem, a[1], wide)?
            };
            if (ordinal && a[4] as u32 != 0)
                || matches!(id, CompareInsensitiveA | CompareInsensitiveW)
            {
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
            } as u64)
        }
    }
}
