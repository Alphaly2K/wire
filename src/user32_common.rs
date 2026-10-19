/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 *
 * This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public License
 * as published by the Free Software Foundation; either version 2.1
 * of the License, or (at your option) any later version.
 * This library is distributed without any warranty; see COPYING.LIB.
 */
//! Wine 10.0 window-text and dialog-control message flow, with wire's logical
//! window state, guest callbacks and checked pointers replacing NT/backend
//! calls. Geometry/rectangle utilities follow the Win32 contracts directly.
use crate::mem::GuestMem;
use crate::user32::User32;

pub(crate) fn copy_text(
    mem: &GuestMem,
    p: u32,
    capacity: u32,
    text: &[u16],
    wide: bool,
) -> Result<u32, String> {
    if p == 0 || capacity == 0 || (capacity as i32) < 0 {
        return Ok(0);
    }
    let mut bytes = if wide {
        text.iter()
            .flat_map(|c| c.to_le_bytes())
            .collect::<Vec<_>>()
    } else {
        crate::codepage::encode(&String::from_utf16_lossy(text), 932)
            .map_err(|e| format!("text encoding {e}"))?
            .0
    };
    let unit = if wide { 2 } else { 1 };
    let count = (bytes.len() / unit).min(capacity as usize - 1);
    bytes.truncate(count * unit);
    bytes.resize((count + 1) * unit, 0);
    mem.write_bytes(p, &bytes).map_err(|e| e.to_string())?;
    Ok(count as u32)
}
pub(crate) fn default_text(
    wide: bool,
    a: &crate::api::Args,
    mem: &GuestMem,
    user: &mut User32,
) -> Result<Option<u32>, String> {
    if !matches!(a[1], 0xc..=0xe) {
        return Ok(None);
    }
    let Some(w) = user.windows.get_mut(&a[0]) else {
        return Ok(Some(0));
    };
    Ok(Some(match a[1] {
        0xc => {
            w.text = crate::menu::read_text(mem, a[3], wide)
                .map_err(|e| format!("WM_SETTEXT {e}"))?
                .unwrap_or_default();
            1
        }
        0xd => copy_text(mem, a[3], a[2], &w.text, wide)?,
        _ => {
            if wide {
                w.text.len() as u32
            } else {
                crate::codepage::encode(&String::from_utf16_lossy(&w.text), 932)
                    .map_err(|e| format!("text encoding {e}"))?
                    .0
                    .len() as u32
            }
        }
    }))
}
pub(crate) fn words(mem: &GuestMem, p: u32, count: usize) -> Result<Vec<i32>, String> {
    let mut bytes = vec![0; count * 4];
    mem.read_bytes(p, &mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes(*b))
        .collect())
}
pub(crate) fn write_words(mem: &GuestMem, p: u32, words: &[i32]) -> Result<(), String> {
    mem.write_bytes(
        p,
        &words
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .map_err(|e| e.to_string())
}
