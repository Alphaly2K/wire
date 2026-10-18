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
use crate::api::{ApiLogger, ApiState, ApiTable};
use crate::cpu::{Cpu, GuestCallOutcome};
use crate::mem::GuestMem;
use crate::thread::Sched;

type ResultValue = Result<(GuestCallOutcome, String), String>;
fn value(n: u32, note: &str) -> ResultValue {
    Ok((GuestCallOutcome::Returned(n), note.into()))
}
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
    state: &mut ApiState,
) -> Result<Option<u32>, String> {
    if !matches!(a[1], 0xc..=0xe) {
        return Ok(None);
    }
    let Some(w) = state.user32.windows.get_mut(&a[0]) else {
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
// Adapted 2026-10-06 from Hangover Wine win32u/defwnd.c's
// handle_window_pos_changed (LGPL-2.1-or-later): Copyright 1993, 1996
// Alexandre Julliard; 1995 Alex Korobka; 2022 Jacek Caban for CodeWeavers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn default_windowpos_changed(
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    let flags = mem
        .read_u32(a[3].checked_add(24).ok_or("WINDOWPOS overflow")?)
        .map_err(|e| e.to_string())?;
    let Some(w) = state.user32.windows.get(&a[0]) else {
        return value(0, "window no longer exists");
    };
    let rect = w.rect;
    let f = w.frame();
    let pack = |x: i32, y: i32| (x as u32 & 0xffff) | ((y as u32) << 16);
    if flags & 0x1000 == 0 {
        let proc = w.wndproc;
        let result = crate::user32::invoke(
            cpu,
            mem,
            api,
            state,
            log,
            sched,
            proc,
            &[a[0], 3, 0, pack(rect[0] + f[0], rect[1] + f[1])],
            max_instr,
        )?;
        if !matches!(result, GuestCallOutcome::Returned(_)) {
            return Ok((result, "WM_MOVE transferred".into()));
        }
    }
    if (flags & 0x800 == 0 || flags & 0x8000 != 0) && state.user32.windows.contains_key(&a[0]) {
        let w = &state.user32.windows[&a[0]];
        let (wp, lp) = if w.style & 0x20000000 != 0 {
            (1, 0)
        } else {
            (
                (w.style & 0x01000000 != 0) as u32 * 2,
                pack(w.client_size()[0], w.client_size()[1]),
            )
        };
        let result = crate::user32::invoke(
            cpu,
            mem,
            api,
            state,
            log,
            sched,
            w.wndproc,
            &[a[0], 5, wp, lp],
            max_instr,
        )?;
        if !matches!(result, GuestCallOutcome::Returned(_)) {
            return Ok((result, "WM_SIZE transferred".into()));
        }
    }
    value(0, "default WM_MOVE/WM_SIZE")
}
