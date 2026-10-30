/* Windows formatting rules adapted from Hangover Wine user32/wsprintf.c,
 * commit 3ea3a14941b6a122b892c84141ce9662c008081e.
 * Copyright 1996 Alexandre Julliard. LGPL-2.1-or-later; wire selects 2.1.
 * See third_party/hangover/README.md and COPYING.LIB. */
//! Bounded Windows formatting with lazy guest arguments. No host printf.
pub(crate) trait Memory {
    type Error;
    fn add(&self, pointer: u64, offset: u64) -> Result<u64, Self::Error>;
    fn read(&self, pointer: u64, output: &mut [u8]) -> Result<(), Self::Error>;
    fn write(&mut self, pointer: u64, bytes: &[u8]) -> Result<(), Self::Error>;
    fn argument(&mut self, wide: bool) -> Result<u64, Self::Error>;
    fn pointer_width(&self) -> usize;
    fn invalid(&self, message: String) -> Self::Error;
    fn read_int(&self, pointer: u64, size: usize) -> Result<u64, Self::Error> {
        let mut bytes = [0; 8];
        self.read(pointer, &mut bytes[..size])?;
        Ok(u64::from_le_bytes(bytes))
    }
}

fn string<M: Memory>(mem: &M, p: u64, wide: bool, limit: usize) -> Result<Vec<u16>, M::Error> {
    let mut out = Vec::new();
    for i in 0..limit {
        let at = mem.add(p, (i * if wide { 2 } else { 1 }) as u64)?;
        let c = mem.read_int(at, if wide { 2 } else { 1 })? as u16;
        if c == 0 {
            break;
        }
        out.push(c);
    }
    Ok(out)
}
fn convert(text: Vec<u16>, from_wide: bool, to_wide: bool) -> Result<Vec<u16>, String> {
    if from_wide == to_wide {
        return Ok(text);
    }
    if to_wide {
        crate::codepage::decode_lossy(&text.iter().map(|&c| c as u8).collect::<Vec<_>>(), 932)
            .map(|s| s.encode_utf16().collect())
            .map_err(|e| format!("format conversion: {e}"))
    } else {
        crate::codepage::encode(&String::from_utf16_lossy(&text), 932)
            .map(|(b, _)| b.into_iter().map(u16::from).collect())
            .map_err(|e| format!("format conversion: {e}"))
    }
}
pub(crate) fn format<M: Memory>(
    mem: &mut M,
    dst: u64,
    spec: u64,
    wide: bool,
) -> Result<u32, M::Error> {
    let fmt = string(mem, spec, wide, 1 << 20)?;
    let mut out = Vec::new();
    let mut i = 0;
    while i < fmt.len() && out.len() < 1023 {
        let c = fmt[i];
        i += 1;
        if c != b'%' as u16 {
            out.push(c);
            continue;
        }
        if fmt.get(i) == Some(&(b'%' as u16)) {
            out.push(b'%' as u16);
            i += 1;
            continue;
        }
        let take = |i: &mut usize, c: u8| {
            if fmt.get(*i) == Some(&(c as u16)) {
                *i += 1;
                true
            } else {
                false
            }
        };
        let left = take(&mut i, b'-');
        let prefix = take(&mut i, b'#');
        let zero = take(&mut i, b'0') && !left;
        let number = |i: &mut usize| {
            let mut n = 0usize;
            while let Some(&c) = fmt
                .get(*i)
                .filter(|&&c| (b'0' as u16..=b'9' as u16).contains(&c))
            {
                n = (n * 10 + (c - b'0' as u16) as usize).min(1023);
                *i += 1;
            }
            n
        };
        let mut width = number(&mut i);
        let precision = if take(&mut i, b'.') {
            number(&mut i)
        } else {
            0
        };
        let long = take(&mut i, b'l');
        let short = !long && take(&mut i, b'h');
        let w = !long && !short && take(&mut i, b'w');
        let intptr = !long && !short && !w && take(&mut i, b'I');
        let i64 = intptr && fmt.get(i..i + 2) == Some(&[b'6' as u16, b'4' as u16]);
        let i32 = intptr && fmt.get(i..i + 2) == Some(&[b'3' as u16, b'2' as u16]);
        if i64 || i32 {
            i += 2;
        }
        let Some(&kind) = fmt.get(i) else { break };
        if !b"cCsSdiuxXp".iter().any(|&b| kind == b as u16) {
            continue; // unknown specifier prints normally; consumes no argument
        }
        i += 1;
        let kind = kind as u8;
        let numeric = matches!(kind, b'd' | b'i' | b'u' | b'x' | b'X' | b'p');
        let full = numeric && kind != b'p' && (i64 || (intptr && !i32 && mem.pointer_width() == 8));
        let arg = mem.argument(full)?;
        let mut field = if numeric {
            let value = if full || kind == b'p' {
                arg
            } else {
                arg as u32 as u64
            };
            let s = match kind {
                b'd' | b'i' => {
                    if full {
                        (value as i64).to_string()
                    } else {
                        (arg as i32).to_string()
                    }
                }
                b'x' => format!("{value:x}"),
                b'X' | b'p' => format!("{value:X}"),
                _ => value.to_string(),
            };
            if kind == b'p' {
                width = mem.pointer_width() * 2;
            }
            let mut digits: Vec<u16> = s.encode_utf16().collect();
            let wanted = precision.max(if zero || kind == b'p' { width } else { 0 });
            if digits.len() < wanted {
                let at = usize::from(digits.first() == Some(&(b'-' as u16)));
                digits.splice(
                    at..at,
                    std::iter::repeat_n(b'0' as u16, wanted - digits.len()),
                );
            }
            if prefix && matches!(kind, b'x' | b'X' | b'p') {
                digits.splice(
                    0..0,
                    [b'0' as u16, if kind == b'x' { b'x' } else { b'X' } as u16],
                );
            }
            digits
        } else {
            let source_wide = match (wide, kind) {
                (false, b'c') => long,
                (false, b'C') => !short,
                (true, b'c') => !short,
                (true, b'C') => long,
                (false, b's') => long || w,
                (false, b'S') => !short && !w,
                (true, b's') => !short || w,
                (true, b'S') => long || w,
                _ => unreachable!(),
            };
            let text = if matches!(kind, b'c' | b'C') {
                vec![if source_wide {
                    arg as u16
                } else {
                    arg as u8 as u16
                }]
            } else if arg == 0 {
                "(null)".encode_utf16().collect()
            } else {
                string(
                    mem,
                    arg,
                    source_wide,
                    if precision == 0 { 1023 } else { precision },
                )?
            };
            let mut text = convert(text, source_wide, wide).map_err(|error| mem.invalid(error))?;
            if matches!(kind, b's' | b'S') && precision != 0 {
                text.truncate(precision);
            }
            text
        };
        if width > field.len() {
            let padding = std::iter::repeat_n(b' ' as u16, width - field.len());
            if left {
                field.extend(padding);
            } else {
                field.splice(0..0, padding);
            }
        }
        out.extend(field.into_iter().take(1023 - out.len()));
    }
    let n = out.len();
    out.push(0);
    let bytes = if wide {
        out.into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    } else {
        out.into_iter().map(|c| c as u8).collect()
    };
    mem.write(dst, &bytes)?;
    Ok(if n == 1023 { u32::MAX } else { n as u32 })
}
