//! Shared PE32 CRT byte operations and bounded unsigned parsing.
pub(crate) trait Memory {
    type Error;
    fn invalid(message: &'static str) -> Self::Error;
    fn check(&self, pointer: u32, length: usize) -> Result<(), Self::Error>;
    fn read_bytes(&self, pointer: u32, bytes: &mut [u8]) -> Result<(), Self::Error>;
    fn write_bytes(&mut self, pointer: u32, bytes: &[u8]) -> Result<(), Self::Error>;
    fn read_u8(&self, pointer: u32) -> Result<u8, Self::Error> {
        let mut byte = [0];
        self.read_bytes(pointer, &mut byte)?;
        Ok(byte[0])
    }
    fn write_u32(&mut self, pointer: u32, value: u32) -> Result<(), Self::Error> {
        self.write_bytes(pointer, &value.to_le_bytes())
    }
    fn compare(&self, left: u32, right: u32, length: u32) -> Result<u32, Self::Error> {
        self.check(left, length as usize)?;
        self.check(right, length as usize)?;
        let mut a = [0; 4096];
        let mut b = [0; 4096];
        let mut offset = 0;
        while offset < length {
            let count = (length - offset).min(4096) as usize;
            self.read_bytes(left + offset, &mut a[..count])?;
            self.read_bytes(right + offset, &mut b[..count])?;
            if let Some(value) = a[..count]
                .iter()
                .zip(&b[..count])
                .find_map(|(&a, &b)| (a != b).then_some((a as i32 - b as i32) as u32))
            {
                return Ok(value);
            }
            offset += count as u32;
        }
        Ok(0)
    }
}
pub(crate) fn call<M: Memory>(
    name: &str,
    args: &[u32; 16],
    mem: &mut M,
    unix_seconds: u64,
) -> Result<Option<u32>, M::Error> {
    let value = match name {
        "time" | "_time32" => {
            let n = unix_seconds;
            let value = if n > i32::MAX as u64 {
                u32::MAX
            } else {
                n as u32
            };
            if args[0] != 0 {
                mem.write_u32(args[0], value)?;
            }
            value
        }
        "_mbsinc" => {
            let first = mem.read_u8(args[0])?;
            let lead = matches!(first, 0x81..=0x9f | 0xe0..=0xfc);
            let pair = lead
                && mem.read_u8(
                    args[0]
                        .checked_add(1)
                        .ok_or_else(|| M::invalid("_mbsinc address overflow"))?,
                )? != 0;
            args[0]
                .checked_add(if pair { 2 } else { 1 })
                .ok_or_else(|| M::invalid("_mbsinc address overflow"))?
        }
        "_mbsnbcpy" => {
            // Hangover Wine mbcs.c, LGPL-2.1-or-later, Alexandre Julliard
            // (1999), Jon Griffths (2000). Checked CP932 guest adaptation.
            if args[2] == 0 {
                return Ok(Some(args[0]));
            }
            if args[2] > 1 << 26 {
                return Err(M::invalid("_mbsnbcpy size too large"));
            }
            let mut bytes = vec![0; args[2] as usize];
            let mut lead = false;
            let mut copied = 0;
            for i in 0..args[2] {
                let b = mem.read_u8(
                    args[1]
                        .checked_add(i)
                        .ok_or_else(|| M::invalid("_mbsnbcpy address overflow"))?,
                )?;
                if b == 0 {
                    break;
                }
                lead = !lead && matches!(b, 0x81..=0x9f | 0xe0..=0xfc);
                bytes[i as usize] = b;
                copied += 1;
            }
            if lead {
                bytes[copied - 1] = 0;
            }
            mem.write_bytes(args[0], &bytes)?;
            args[0]
        }
        "memcpy" | "memmove" => {
            let n = args[2] as usize;
            if n > 1 << 26 {
                return Err(M::invalid("CRT memory range too large"));
            }
            let mut tmp = vec![0u8; n];
            mem.read_bytes(args[1], &mut tmp)?;
            mem.write_bytes(args[0], &tmp)?;
            args[0]
        }
        "memset" => {
            let n = args[2] as usize;
            if n > 1 << 26 {
                return Err(M::invalid("CRT memory range too large"));
            }
            let tmp = vec![args[1] as u8; n];
            mem.write_bytes(args[0], &tmp)?;
            args[0]
        }
        "memcmp" => {
            if args[2] == 0 {
                0
            } else {
                if args[2] > 1 << 26 {
                    return Err(M::invalid("memcmp range too large"));
                }
                mem.compare(args[0], args[1], args[2])?
            }
        }
        "toupper" | "tolower" => {
            // The current CRT supports the default C locale. Values outside
            // ASCII (including EOF) retain their exact integer value.
            match (name, args[0]) {
                ("toupper", 0x61..=0x7a) => args[0] - 32,
                ("tolower", 0x41..=0x5a) => args[0] + 32,
                _ => args[0],
            }
        }
        "strcmp" | "strncmp" => {
            let limit = if name == "strncmp" { args[2] } else { u32::MAX };
            let mut result = 0;
            for i in 0..limit {
                let left = mem.read_u8(
                    args[0]
                        .checked_add(i)
                        .ok_or_else(|| M::invalid("strcmp address overflow"))?,
                )?;
                let right = mem.read_u8(
                    args[1]
                        .checked_add(i)
                        .ok_or_else(|| M::invalid("strcmp address overflow"))?,
                )?;
                if left != right {
                    result = (left as i32 - right as i32) as u32;
                    break;
                }
                if left == 0 {
                    break;
                }
            }
            result
        }
        "strlen" => {
            let mut len = None;
            for i in 0..1 << 20 {
                if mem.read_u8(
                    args[0]
                        .checked_add(i)
                        .ok_or_else(|| M::invalid("strlen address overflow"))?,
                )? == 0
                {
                    len = Some(i);
                    break;
                }
            }
            len.ok_or_else(|| M::invalid("unterminated strlen source"))?
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

pub(crate) struct Parsed {
    pub value: u32,
    pub end: u32,
    pub error: Option<u32>,
}
pub(crate) fn strtoul<M: Memory>(mem: &M, pointer: u32, mut base: u32) -> Result<Parsed, M::Error> {
    let mut bytes = Vec::new();
    for i in 0..1 << 20 {
        let p = pointer
            .checked_add(i)
            .ok_or_else(|| M::invalid("strtoul address overflow"))?;
        let b = mem.read_u8(p)?;
        if b == 0 {
            break;
        }
        bytes.push(b);
    }
    if bytes.len() == 1 << 20 {
        return Err(M::invalid("unterminated strtoul source"));
    }
    let digit = |b: u8| match b {
        b'0'..=b'9' => Some((b - b'0') as u32),
        b'a'..=b'z' => Some((b - b'a' + 10) as u32),
        b'A'..=b'Z' => Some((b - b'A' + 10) as u32),
        _ => None,
    };
    let mut pos = 0;
    while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
        pos += 1;
    }
    let negative = bytes.get(pos) == Some(&b'-');
    if bytes.get(pos).is_some_and(|b| matches!(b, b'+' | b'-')) {
        pos += 1;
    }
    let prefix = bytes.get(pos) == Some(&b'0')
        && bytes.get(pos + 1).is_some_and(|b| matches!(b, b'x' | b'X'))
        && bytes
            .get(pos + 2)
            .and_then(|&b| digit(b))
            .is_some_and(|n| n < 16);
    if (base == 0 || base == 16) && prefix {
        base = 16;
        pos += 2;
    }
    if base == 0 {
        base = if bytes.get(pos) == Some(&b'0') { 8 } else { 10 };
    }
    let start = pos;
    let mut n = 0u32;
    let mut overflow = false;
    if (2..=36).contains(&base) {
        while let Some(d) = bytes.get(pos).and_then(|&b| digit(b)).filter(|&d| d < base) {
            if let Some(value) = n.checked_mul(base).and_then(|v| v.checked_add(d)) {
                n = value;
            } else {
                overflow = true;
            }
            pos += 1;
        }
    }
    Ok(Parsed {
        value: if overflow {
            u32::MAX
        } else if negative {
            n.wrapping_neg()
        } else {
            n
        },
        end: pointer
            .checked_add(if pos == start { 0 } else { pos as u32 })
            .ok_or_else(|| M::invalid("strtoul end pointer overflow"))?,
        error: if overflow {
            Some(34)
        } else if !(2..=36).contains(&base) {
            Some(22)
        } else {
            None
        },
    })
}
