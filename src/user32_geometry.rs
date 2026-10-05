/*
 * Adapted 2026-10-06 from Hangover Wine win32u/defwnd.c, fixed commit
 * 3ea3a14941b6a122b892c84141ce9662c008081e.
 * Copyright 1993, 1996 Alexandre Julliard
 * Copyright 1995 Alex Korobka
 * Copyright 2022 Jacek Caban for CodeWeavers
 * GNU Lesser General Public License, version 2.1 or later. No warranty.
 * Original excerpt and license: third_party/hangover/.
 */
//! Shared nonclient geometry for the initial 96-DPI desktop profile.
use crate::{api::ApiState, mem::GuestMem};
use std::collections::BTreeSet;

pub(crate) fn frame(style: u32, ex: u32, menu: bool, scroll: bool) -> [i32; 4] {
    let mut border = if ex & (0x20000 | 1) == 0x20000 {
        1
    } else if ex & 1 != 0 || style & (0x40000 | 0x400000) != 0 {
        2
    } else {
        0
    };
    if style & 0x40000 != 0 {
        border += 1;
    } // border width 1, padded border 0
    if style & (0x800000 | 0x400000) != 0 || ex & 1 != 0 {
        border += 1;
    }
    if ex & 0x200 != 0 {
        border += 2;
    }
    let caption = if style & 0xc00000 == 0xc00000 {
        if ex & 0x80 != 0 {
            18
        } else {
            23
        }
    } else {
        0
    };
    let mut result = [
        border,
        border + caption + if menu { 19 } else { 0 },
        border,
        border,
    ];
    if scroll {
        if style & 0x200000 != 0 {
            result[if ex & 0x4000 != 0 { 0 } else { 2 }] += 17;
        }
        if style & 0x100000 != 0 {
            result[3] += 17;
        }
    }
    result
}
pub(crate) fn client_origin(state: &ApiState, mut h: u32) -> Result<[i32; 2], String> {
    let mut seen = BTreeSet::new();
    let mut result = [0i32; 2];
    while h != 0 {
        if !seen.insert(h) {
            return Err("cyclic parent coordinates".into());
        }
        let w = state
            .user32
            .windows
            .get(&h)
            .ok_or("invalid window coordinates")?;
        let f = w.frame();
        for i in 0..2 {
            result[i] = result[i].wrapping_add(w.rect[i]).wrapping_add(f[i]);
        }
        if w.style & 0x40000000 == 0 {
            break;
        }
        h = w.parent;
    }
    Ok(result)
}
pub(crate) fn adjust(
    name: &str,
    a: &crate::api::Args,
    mem: &GuestMem,
    state: &mut ApiState,
) -> Option<Result<u32, String>> {
    if !matches!(
        name,
        "AdjustWindowRect" | "AdjustWindowRectEx" | "AdjustWindowRectExForDpi"
    ) {
        return None;
    }
    Some((|| {
        if name.ends_with("ForDpi") && a[4] != 96 {
            state.last_error = 120;
            return Ok(0);
        }
        let ex = if name == "AdjustWindowRect" { 0 } else { a[3] };
        let mut bytes = [0; 16];
        mem.read_bytes(a[0], &mut bytes)
            .map_err(|e| e.to_string())?;
        mem.ptr(a[0], 16, true).map_err(|e| e.to_string())?;
        let f = frame(a[1], ex, a[2] != 0, false);
        for i in 0..4 {
            let n = i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
            bytes[i * 4..i * 4 + 4].copy_from_slice(
                &if i < 2 {
                    n.wrapping_sub(f[i])
                } else {
                    n.wrapping_add(f[i])
                }
                .to_le_bytes(),
            );
        }
        mem.write_bytes(a[0], &bytes).map_err(|e| e.to_string())?;
        Ok(1)
    })())
}
