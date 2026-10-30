/* PathRemoveFileSpec, PathAddExtension, PathUnquoteSpaces and PathCanonicalize A/W adapted from
 * Hangover's Wine kernelbase/path.c,
 * commit 3ea3a14941b6a122b892c84141ce9662c008081e (LGPL-2.1-or-later).
 * Copyright 2018 Nikolay Sivov; 2018 Zhiyi Zhang;
 * Copyright 2021-2023 Zebediah Figura. Wire selects LGPL 2.1.
 * See third_party/hangover/path.c and COPYING.LIB for original notices.
 */
//! Pure path transformations shared by legacy and Wasm memory adapters.
pub(crate) fn canonicalize(path: &[u16]) -> Vec<u16> {
    const SLASH: u16 = 92;
    const DOT: u16 = 46;
    const COLON: u16 = 58;
    if path.is_empty() {
        return vec![SLASH];
    }
    let at = |n| path.get(n).copied().unwrap_or(0);
    let mut out = vec![0; path.len() + 4];
    let (mut src, mut dst) = (0, 0);
    if at(0) == SLASH {
        out[0] = SLASH;
        src = 1;
        dst = 1;
    } else if at(1) == COLON {
        out[..2].copy_from_slice(&path[..2]);
        src = 2;
        dst = 2;
        if at(src) == SLASH {
            out[dst] = SLASH;
            src += 1;
            dst += 1;
        }
    }
    while src < path.len() {
        if at(src) == DOT
            && at(src + 1) == SLASH
            && (src == 0 || matches!(at(src - 1), SLASH | COLON))
        {
            src += 2;
        } else if at(src) == DOT && at(src + 1) == DOT && dst != 0 && out[dst - 1] == SLASH {
            out[dst] = 0;
            if dst > 1 && (out[dst - 2] != SLASH || dst > 2) {
                if out[dst - 2] == COLON
                    && (dst > 3 || out.get(dst.wrapping_sub(3)) == Some(&COLON))
                {
                    dst -= 2;
                    while dst > 0 && out[dst] != SLASH {
                        dst -= 1;
                    }
                    if out[dst] == SLASH {
                        dst += 1;
                    } else {
                        dst = 0;
                    }
                } else if out[dst - 2] != COLON {
                    let unc_share = out[..dst].starts_with(&[SLASH, SLASH])
                        && out[2..dst].iter().filter(|&&c| c == SLASH).count() == 1;
                    if !unc_share {
                        dst -= 2;
                    }
                }
            }
            while dst > 0 && out[dst] != SLASH {
                dst -= 1;
            }
            if dst == 0 {
                out[dst] = SLASH;
                dst += 1;
                src += 1;
            }
            src += 2;
        } else {
            out[dst] = at(src);
            dst += 1;
            src += 1;
        }
    }
    if dst == 2 && out[1] == COLON {
        out[dst] = SLASH;
        dst += 1;
    }
    out.truncate(dst);
    out
}

pub(crate) fn remove_file_spec<E>(
    wide: bool,
    mut read: impl FnMut(u32) -> Result<u16, E>,
    invalid: impl Fn() -> E,
) -> Result<Option<u32>, E> {
    let mut chars = Vec::new();
    let mut offset = 0;
    loop {
        if offset >= 32768 {
            return Err(invalid());
        }
        let c = read(offset)?;
        if c == 0 {
            break;
        }
        chars.push((offset, c));
        offset += if wide { 2 } else { 1 };
        // CP932 trail byte 0x5c is part of a character, not '\\'.
        if !wide && matches!(c, 0x81..=0x9f | 0xe0..=0xfc) {
            let trail = read(offset)?;
            if trail == 0 {
                break;
            }
            offset += 1;
        }
    }
    // Hangover's pinned Wine kernelbase/path.c root and separator
    // rules, with indices into checked guest characters replacing
    // native pointers. Keep ANSI/Wide extended-prefix differences.
    let at = |i: usize| chars.get(i).map_or(0, |(_, c)| *c);
    let drive = |i| matches!(at(i), 65..=90 | 97..=122) && at(i + 1) == 58;
    let prefix = |text: &str, insensitive: bool| {
        text.bytes().enumerate().all(|(i, c)| {
            let ch = at(i);
            ch == c as u16 || (insensitive && ch <= 127 && (ch as u8).eq_ignore_ascii_case(&c))
        })
    };
    let volume = wide
        && prefix("\\\\?\\Volume", true)
        && (0..38).all(|i| match i {
            0 => at(10 + i) == 123,
            9 | 14 | 19 | 24 => at(10 + i) == 45,
            37 => at(10 + i) == 125,
            _ => matches!(at(10 + i), 48..=57 | 65..=70 | 97..=102),
        });
    let mut root = if volume {
        Some(48)
    } else if wide && prefix("\\\\?\\", false) && drive(4) {
        Some(6)
    } else if drive(0) {
        Some(2)
    } else {
        None
    };
    if let Some(r) = root.as_mut() {
        if at(*r) == 92 {
            *r += 1;
        }
    } else {
        let mut r = usize::from(at(0) == 92);
        if at(r + 1) != 63 {
            if at(r) == 92 {
                r += 1;
            }
            if r > 1 && drive(r) {
                r += 2;
            }
            if at(r) == 92 && at(r + 1) != 0 && at(r + 1) != 92 {
                r += 1;
            }
        }
        root = Some(r);
    }
    let root = root.unwrap();
    let mut end_index = chars
        .iter()
        .enumerate()
        .skip(root)
        .filter(|(_, (_, c))| *c == 92)
        .map(|(i, _)| i)
        .next_back()
        .unwrap_or(root);
    if end_index != root && at(end_index - 1) == 92 {
        end_index -= 1;
    }
    if at(end_index) == 0 {
        return Ok(None);
    }
    Ok(Some(chars[end_index].0))
}

pub(crate) fn unquote(text: &str) -> Option<&str> {
    (text.len() >= 2 && text.starts_with('"') && text.ends_with('"'))
        .then(|| &text[1..text.len() - 1])
}

pub(crate) fn extension(
    text: &str,
    ext: &str,
    add: bool,
    wide: bool,
) -> Result<Option<String>, u32> {
    let mut text = text.to_owned();
    let start = text.rfind(['\\', '/', ' ']).map_or(0, |n| n + 1);
    if let Some(dot) = text[start..].rfind('.') {
        if add {
            return Ok(None);
        }
        text.truncate(start + dot);
    }
    text.push_str(ext);
    let count = if wide {
        text.encode_utf16().count()
    } else {
        crate::codepage::encode(&text, 932)?.0.len()
    };
    if count >= 260 {
        return Err(206);
    }
    Ok(Some(text))
}
