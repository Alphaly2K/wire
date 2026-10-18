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
use std::collections::BTreeSet;

type ResultValue = Result<(GuestCallOutcome, String), String>;
fn value(n: u32, note: &str) -> ResultValue {
    Ok((GuestCallOutcome::Returned(n), note.into()))
}
fn failure(state: &mut ApiState, error: u32) -> ResultValue {
    state.last_error = error;
    value(0, &format!("USER32 error {error}"))
}
fn copy_text(
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
    name: &str,
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
    let wide = name.ends_with('W');
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
fn child(state: &ApiState, parent: u32, id: u32) -> Option<u32> {
    state.user32.windows.iter().find_map(|(&h, w)| {
        (w.parent == parent && w.style & 0x40000000 != 0 && w.menu == id).then_some(h)
    })
}
fn origin(state: &ApiState, h: u32) -> Result<[i32; 2], String> {
    crate::user32_geometry::client_origin(state, h)
}
fn words(mem: &GuestMem, p: u32, count: usize) -> Result<Vec<i32>, String> {
    let mut bytes = vec![0; count * 4];
    mem.read_bytes(p, &mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes(*b))
        .collect())
}
fn write_words(mem: &GuestMem, p: u32, words: &[i32]) -> Result<(), String> {
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
pub(crate) fn unimplemented(name: &str) -> bool {
    matches!(
        name,
        "DialogBoxIndirectParamW"
            | "CreateDialogIndirectParamW"
            | "MapDialogRect"
            | "IsDialogMessageW"
            | "CreateIconIndirect"
            | "DestroyIcon"
            | "GetWindowPlacement"
            | "SetWindowPlacement"
            | "GetScrollInfo"
            | "SetScrollInfo"
            | "LockWindowUpdate"
    )
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    name: &str,
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Option<ResultValue> {
    if !matches!(
        name,
        "GetWindowTextW"
            | "GetWindowTextA"
            | "GetWindowTextLengthW"
            | "GetWindowTextLengthA"
            | "SetWindowTextW"
            | "SetWindowTextA"
            | "GetDlgItemTextW"
            | "GetDlgItemTextA"
            | "SetDlgItemTextW"
            | "SetDlgItemTextA"
            | "SendDlgItemMessageW"
            | "SendDlgItemMessageA"
            | "GetDlgItem"
            | "GetDlgCtrlID"
            | "GetDlgItemInt"
            | "SetDlgItemInt"
            | "GetClassNameW"
            | "GetClassNameA"
            | "IsWindowEnabled"
            | "EnableWindow"
            | "SetParent"
            | "MoveWindow"
            | "SetWindowPos"
            | "GetFocus"
            | "SetFocus"
            | "GetActiveWindow"
            | "SetActiveWindow"
            | "GetForegroundWindow"
            | "SetForegroundWindow"
            | "GetLastActivePopup"
            | "SetCapture"
            | "ReleaseCapture"
            | "GetCapture"
            | "MapWindowPoints"
            | "IsIconic"
            | "IsZoomed"
            | "GetSystemMetrics"
            | "GetDoubleClickTime"
            | "GetDialogBaseUnits"
            | "EnumChildWindows"
            | "MonitorFromWindow"
            | "MonitorFromPoint"
            | "MonitorFromRect"
            | "GetMonitorInfoW"
            | "GetMonitorInfoA"
            | "DialogBoxIndirectParamW"
            | "CreateDialogIndirectParamW"
            | "MapDialogRect"
            | "IsDialogMessageW"
            | "CreateIconIndirect"
            | "DestroyIcon"
            | "GetWindowPlacement"
            | "SetWindowPlacement"
            | "GetScrollInfo"
            | "SetScrollInfo"
            | "LockWindowUpdate"
    ) {
        return None;
    }
    Some((|| -> ResultValue {
        if unimplemented(name) {
            return failure(state, 120);
        }
        // Dialog/control wrappers use the same application WndProc boundary
        // as SendMessage. Nonlocal transfers are propagated to the caller.
        if name.contains("Text")
            || name.contains("DlgItemMessage")
            || matches!(name, "GetDlgItemInt" | "SetDlgItemInt")
        {
            let is_dialog = name.contains("DlgItem");
            let hwnd = if is_dialog {
                child(state, a[0], a[1]).unwrap_or(0)
            } else {
                a[0]
            };
            if hwnd == 0 || !state.user32.windows.contains_key(&hwnd) {
                return failure(state, 1400);
            }
            let w = &state.user32.windows[&hwnd];
            if w.tid != sched.current_tid() {
                return failure(state, 120);
            }
            let proc = w.wndproc;
            let mut output = None;
            let (message, wp, lp) = if name.contains("ItemMessage") {
                (a[2], a[3], a[4])
            } else if name == "GetDlgItemInt" {
                if a[2] != 0 {
                    mem.write_u32(a[2], 0).map_err(|e| e.to_string())?;
                }
                let p = state.scratch_alloc(mem, 60, 2);
                if p == 0 {
                    return failure(state, 8);
                }
                output = Some(p);
                (0xd, 30, p)
            } else if name == "SetDlgItemInt" {
                let text = if a[3] != 0 {
                    (a[2] as i32).to_string()
                } else {
                    a[2].to_string()
                };
                let p = state.scratch_alloc(mem, (text.len() as u32 + 1) * 2, 2);
                if p == 0 {
                    return failure(state, 8);
                }
                mem.write_bytes(
                    p,
                    &text
                        .encode_utf16()
                        .chain([0])
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>(),
                )
                .map_err(|e| e.to_string())?;
                (0xc, 0, p)
            } else if name.contains("Length") {
                (0xe, 0, 0)
            } else if name.starts_with("Set") {
                (0xc, 0, a[if is_dialog { 2 } else { 1 }])
            } else {
                (
                    0xd,
                    a[if is_dialog { 3 } else { 2 }],
                    a[if is_dialog { 2 } else { 1 }],
                )
            };
            let result = crate::user32::invoke(
                cpu,
                mem,
                api,
                state,
                log,
                sched,
                proc,
                &[hwnd, message, wp, lp],
                max_instr,
            )?;
            if let (Some(p), GuestCallOutcome::Returned(count)) = (output, &result) {
                let text = mem
                    .read_wstr(p, (*count).min(29) as usize)
                    .map_err(|e| e.to_string())?;
                let s = text.trim_start_matches([' ', '\t', '\r', '\n']);
                let neg = s.starts_with('-');
                let s = s.strip_prefix(['-', '+']).unwrap_or(s);
                let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
                let n = digits.parse::<u64>().ok().and_then(|n| {
                    if a[3] != 0 {
                        let n = i64::try_from(n).ok()?;
                        i32::try_from(if neg { -n } else { n })
                            .ok()
                            .map(|v| v as u32)
                    } else {
                        u32::try_from(n)
                            .ok()
                            .map(|v| if neg { v.wrapping_neg() } else { v })
                    }
                });
                if a[2] != 0 {
                    mem.write_u32(a[2], n.is_some() as u32)
                        .map_err(|e| e.to_string())?;
                }
                return value(n.unwrap_or(0), "dialog decimal conversion");
            }
            return Ok((result, "window/control message callback".into()));
        }
        match name {
            "GetDlgItem" => value(
                child(state, a[0], a[1]).unwrap_or(0),
                "child control lookup",
            ),
            "GetDlgCtrlID" => value(
                state.user32.windows.get(&a[0]).map_or(0, |w| w.menu),
                "control ID",
            ),
            "GetClassNameA" | "GetClassNameW" => {
                let Some(w) = state.user32.windows.get(&a[0]) else {
                    return failure(state, 1400);
                };
                let text: Vec<u16> = state.user32.classes[&w.class].name.encode_utf16().collect();
                value(
                    copy_text(mem, a[1], a[2], &text, name.ends_with('W'))?,
                    "class name",
                )
            }
            "IsWindowEnabled" | "IsIconic" | "IsZoomed" => {
                let mask = match name {
                    "IsWindowEnabled" => 0x08000000,
                    "IsIconic" => 0x20000000,
                    _ => 0x01000000,
                };
                value(
                    state
                        .user32
                        .windows
                        .get(&a[0])
                        .is_some_and(|w| (w.style & mask != 0) != (name == "IsWindowEnabled"))
                        as u32,
                    "logical window state",
                )
            }
            "EnableWindow" => {
                let Some(w) = state.user32.windows.get_mut(&a[0]) else {
                    return failure(state, 1400);
                };
                let disabled = w.style & 0x08000000 != 0;
                if a[1] != 0 {
                    w.style &= !0x08000000;
                } else {
                    w.style |= 0x08000000;
                }
                let proc = w.wndproc;
                if disabled == (a[1] != 0) {
                    let result = crate::user32::invoke(
                        cpu,
                        mem,
                        api,
                        state,
                        log,
                        sched,
                        proc,
                        &[a[0], 0xa, (a[1] != 0) as u32, 0],
                        max_instr,
                    )?;
                    if !matches!(result, GuestCallOutcome::Returned(_)) {
                        return Ok((result, "WM_ENABLE transferred".into()));
                    }
                }
                value(disabled as u32, "window enabled state")
            }
            "SetParent" => {
                if a[1] != 0 && !state.user32.windows.contains_key(&a[1]) {
                    return failure(state, 1400);
                }
                let mut h = a[1];
                let mut seen = BTreeSet::new();
                while h != 0 {
                    if h == a[0] || !seen.insert(h) {
                        return failure(state, 87);
                    }
                    h = state.user32.windows.get(&h).map_or(0, |w| w.parent);
                }
                let screen = origin(state, a[0])?;
                let parent = origin(state, a[1])?;
                let Some(w) = state.user32.windows.get_mut(&a[0]) else {
                    return failure(state, 1400);
                };
                let old = w.parent;
                w.parent = a[1];
                if w.style & 0x40000000 != 0 {
                    w.rect[0] = screen[0].wrapping_sub(parent[0]);
                    w.rect[1] = screen[1].wrapping_sub(parent[1]);
                }
                value(old, "parent changed; style retained")
            }
            "MoveWindow" | "SetWindowPos" => {
                let Some(w) = state.user32.windows.get(&a[0]) else {
                    return failure(state, 1400);
                };
                let proc = w.wndproc;
                let old_rect = w.rect;
                let flags = if name == "MoveWindow" { 0x14 } else { a[6] };
                let offset = if name == "MoveWindow" { 1 } else { 2 };
                let p = state.scratch_alloc(mem, 28, 4);
                if p == 0 {
                    return failure(state, 8);
                }
                write_words(
                    mem,
                    p,
                    &[
                        a[0] as i32,
                        if name == "MoveWindow" { 0 } else { a[1] as i32 },
                        a[offset] as i32,
                        a[offset + 1] as i32,
                        a[offset + 2] as i32,
                        a[offset + 3] as i32,
                        flags as i32,
                    ],
                )?;
                if flags & 0x400 == 0 {
                    let result = crate::user32::invoke(
                        cpu,
                        mem,
                        api,
                        state,
                        log,
                        sched,
                        proc,
                        &[a[0], 0x46, 0, p],
                        max_instr,
                    )?;
                    if !matches!(result, GuestCallOutcome::Returned(_)) {
                        return Ok((result, "WM_WINDOWPOSCHANGING transferred".into()));
                    }
                }
                let pos = words(mem, p, 7)?;
                let mut flags = pos[6] as u32;
                let Some(w) = state.user32.windows.get_mut(&a[0]) else {
                    return failure(state, 1400);
                };
                if flags & 2 == 0 {
                    w.rect[..2].copy_from_slice(&pos[2..4]);
                }
                if flags & 1 == 0 {
                    w.rect[2] = pos[4].max(0);
                    w.rect[3] = pos[5].max(0);
                }
                if flags & 0x40 != 0 {
                    w.style |= 0x10000000;
                }
                if flags & 0x80 != 0 {
                    w.style &= !0x10000000;
                }
                // These internal flags describe actual client changes. Default
                // WndProc consumes them to emit WM_MOVE/WM_SIZE independently.
                if w.rect[..2] == old_rect[..2] {
                    flags |= 0x1000;
                }
                if w.rect[2..] == old_rect[2..] {
                    flags |= 0x800;
                }
                mem.write_u32(p + 24, flags).map_err(|e| e.to_string())?;
                let result = crate::user32::invoke(
                    cpu,
                    mem,
                    api,
                    state,
                    log,
                    sched,
                    proc,
                    &[a[0], 0x47, 0, p],
                    max_instr,
                )?;
                if !matches!(result, GuestCallOutcome::Returned(_)) {
                    return Ok((result, "WM_WINDOWPOSCHANGED transferred".into()));
                }
                value(1, "logical window position")
            }
            "GetFocus" => value(state.user32.focus, "focus"),
            "GetActiveWindow" => value(state.user32.active, "active window"),
            "GetForegroundWindow" => value(state.user32.foreground, "foreground window"),
            "GetCapture" => value(state.user32.capture, "capture"),
            "GetLastActivePopup" => value(a[0], "logical owner popup subset"),
            "SetFocus"
            | "SetActiveWindow"
            | "SetForegroundWindow"
            | "SetCapture"
            | "ReleaseCapture" => {
                let h = if name == "ReleaseCapture" { 0 } else { a[0] };
                if h != 0 && !state.user32.windows.contains_key(&h) {
                    return failure(state, 1400);
                }
                let old = match name {
                    "SetFocus" => state.user32.focus,
                    "SetCapture" | "ReleaseCapture" => state.user32.capture,
                    _ => state.user32.active,
                };
                match name {
                    "SetFocus" => state.user32.focus = h,
                    "SetCapture" | "ReleaseCapture" => state.user32.capture = h,
                    _ => {
                        state.user32.active = h;
                        state.user32.foreground = h;
                    }
                }
                let messages: Vec<_> = match name {
                    "SetFocus" => vec![(old, 8, h, 0), (h, 7, old, 0)],
                    "SetCapture" | "ReleaseCapture" => vec![(old, 0x215, 0, h)],
                    _ => vec![(old, 6, 0, h), (h, 6, 1, old)],
                };
                if old != h {
                    for (hwnd, msg, wp, lp) in messages {
                        if let Some(proc) = state.user32.windows.get(&hwnd).map(|w| w.wndproc) {
                            let result = crate::user32::invoke(
                                cpu,
                                mem,
                                api,
                                state,
                                log,
                                sched,
                                proc,
                                &[hwnd, msg, wp, lp],
                                max_instr,
                            )?;
                            if !matches!(result, GuestCallOutcome::Returned(_)) {
                                return Ok((result, "window state callback transferred".into()));
                            }
                        }
                    }
                }
                value(
                    if matches!(name, "ReleaseCapture" | "SetForegroundWindow") {
                        1
                    } else {
                        old
                    },
                    "logical input ownership",
                )
            }
            "MapWindowPoints" => {
                if a[3] > 4096 {
                    return failure(state, 87);
                }
                let from = origin(state, a[0])?;
                let to = origin(state, a[1])?;
                let delta = [from[0].wrapping_sub(to[0]), from[1].wrapping_sub(to[1])];
                let mut points = words(mem, a[2], a[3] as usize * 2)?;
                for xy in points.as_chunks_mut::<2>().0 {
                    for i in 0..2 {
                        xy[i] = xy[i].wrapping_add(delta[i]);
                    }
                }
                write_words(mem, a[2], &points)?;
                value(
                    (delta[0] as u32 & 0xffff) | ((delta[1] as u32) << 16),
                    "logical coordinate mapping",
                )
            }
            "GetDoubleClickTime" => value(500, "input timing profile"),
            "GetDialogBaseUnits" => value(8 | (16 << 16), "initial fixed dialog font units"),
            "GetSystemMetrics" => {
                let n = match a[0] {
                    0 | 16 | 61 | 78 => 1024,
                    1 | 17 | 62 | 79 => 768,
                    2 | 3 | 9 | 10 | 20 | 21 => 17,
                    4 => 23,
                    5..=8 => 1,
                    11..=14 => 32,
                    15 => 19,
                    28..=31 => 112,
                    32 | 33 => 4,
                    36 | 37 | 48 | 49 => 16,
                    43 => 3,
                    45 | 46 | 68 | 80 => 1,
                    83 => 2,
                    _ => 0,
                };
                value(n, "logical 1024x768 desktop / 96 DPI profile")
            }
            "MonitorFromWindow" | "MonitorFromPoint" | "MonitorFromRect" => {
                value(0x40000001, "single logical monitor")
            }
            "GetMonitorInfoA" | "GetMonitorInfoW" => {
                let size = mem.read_u32(a[1]).map_err(|e| e.to_string())?;
                let wide = name.ends_with('W');
                if a[0] != 0x40000001 || (size != 40 && size != if wide { 104 } else { 72 }) {
                    return failure(state, 87);
                }
                let mut bytes = [size, 0, 0, 1024, 768, 0, 0, 1024, 768, 1]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>();
                if size != 40 {
                    if wide {
                        bytes.extend(
                            "\\\\.\\DISPLAY1"
                                .encode_utf16()
                                .chain(std::iter::repeat(0))
                                .take(32)
                                .flat_map(u16::to_le_bytes),
                        );
                    } else {
                        let mut text = [0; 32];
                        text[..12].copy_from_slice(b"\\\\.\\DISPLAY1");
                        bytes.extend(text);
                    }
                }
                mem.write_bytes(a[1], &bytes).map_err(|e| e.to_string())?;
                value(1, "logical monitor bounds")
            }
            "EnumChildWindows" => {
                let mut seen = BTreeSet::new();
                let mut pending = vec![a[0]];
                let mut children = Vec::new();
                while let Some(parent) = pending.pop() {
                    if !seen.insert(parent) {
                        continue;
                    }
                    for (&h, w) in &state.user32.windows {
                        if w.parent == parent && w.style & 0x40000000 != 0 {
                            children.push(h);
                            pending.push(h);
                        }
                    }
                }
                for h in children {
                    if !state.user32.windows.contains_key(&h) {
                        continue;
                    }
                    let result = crate::user32::invoke(
                        cpu,
                        mem,
                        api,
                        state,
                        log,
                        sched,
                        a[1],
                        &[h, a[2]],
                        max_instr,
                    )?;
                    match result {
                        GuestCallOutcome::Returned(0) => break,
                        GuestCallOutcome::Returned(_) => {}
                        _ => return Ok((result, "child enumeration transferred".into())),
                    }
                }
                value(1, "child window snapshot")
            }
            _ => Err(format!("unhandled USER32 common function {name}")),
        }
    })())
}
