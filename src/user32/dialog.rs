/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 *
 * This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public License
 * as published by the Free Software Foundation; either version 2.1
 * of the License, or (at your option) any later version.
 * This library is distributed without any warranty; see COPYING.LIB.
 */
//! Control messages and snapshot enumeration using typed PE32 API IDs.
use crate::api::{ApiLogger, ApiState, ApiTable};
use crate::cpu::{Cpu, GuestCallOutcome};
use crate::mem::GuestMem;
use crate::thread::Sched;
use crate::win32::catalog::DialogApi as D;
use std::collections::BTreeSet;

type ResultValue = Result<(GuestCallOutcome, String), String>;
fn value(n: u32, note: &str) -> ResultValue {
    Ok((GuestCallOutcome::Returned(n), note.into()))
}
fn failure(state: &mut ApiState, error: u32) -> ResultValue {
    state.last_error = error;
    value(0, &format!("USER32 error {error}"))
}
fn child(state: &ApiState, parent: u32, id: u32) -> Option<u32> {
    state.user32.windows.iter().find_map(|(&h, w)| {
        (w.parent == parent && w.style & 0x40000000 != 0 && w.menu == id).then_some(h)
    })
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn call(
    id: D,
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    if id.unavailable() {
        return failure(state, 120);
    }
    if matches!(id, D::Windows | D::ThreadWindows) {
        return enumerate(id, a, cpu, mem, api, state, log, sched, max_instr);
    }
    // Dialog/control wrappers use the same application WndProc boundary
    // as SendMessage. Nonlocal transfers are propagated to the caller.
    if id.message() {
        let hwnd = child(state, a[0], a[1]).unwrap_or(0);
        if hwnd == 0 || !state.user32.windows.contains_key(&hwnd) {
            return failure(state, 1400);
        }
        let w = &state.user32.windows[&hwnd];
        if w.tid != sched.current_tid() {
            return failure(state, 120);
        }
        let proc = w.wndproc;
        let mut output = None;
        let (message, wp, lp) = if id.send() {
            (a[2], a[3], a[4])
        } else if id == D::GetInteger {
            if a[2] != 0 {
                mem.write_u32(a[2], 0).map_err(|e| e.to_string())?;
            }
            let p = state.scratch_alloc(mem, 60, 2);
            if p == 0 {
                return failure(state, 8);
            }
            output = Some(p);
            (0xd, 30, p)
        } else if id == D::SetInteger {
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
        } else if id.set_text() {
            (0xc, 0, a[2])
        } else {
            (0xd, a[3], a[2])
        };
        let result = crate::x86::process::callbacks::invoke(
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
    match id {
        D::Item => value(
            child(state, a[0], a[1]).unwrap_or(0),
            "child control lookup",
        ),
        D::ControlId => value(
            state.user32.windows.get(&a[0]).map_or(0, |w| w.menu),
            "control ID",
        ),
        D::Children => {
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
                let result = crate::x86::process::callbacks::invoke(
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
        _ => unreachable!("other dialog IDs were handled before child lookup"),
    }
}

#[allow(clippy::too_many_arguments)]
fn enumerate(
    id: D,
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<(GuestCallOutcome, String), String> {
    let (tid, callback, param) = if id == D::ThreadWindows {
        (a[0], a[1], a[2])
    } else {
        (0, a[0], a[1])
    };
    if callback == 0 {
        state.last_error = 87;
        return Ok((
            GuestCallOutcome::Returned(0),
            "null enumeration callback".into(),
        ));
    }
    let windows: Vec<_> = state
        .user32
        .windows
        .iter()
        .rev()
        .filter(|(_, w)| {
            w.tid != 0
                && w.style & 0x40000000 == 0
                && w.parent != 0xfffffffd
                && (tid == 0 || tid == w.tid)
        })
        .map(|(&h, _)| h)
        .collect();
    let mut returned = 1;
    let mut visited = 0;
    for hwnd in windows {
        if !state.user32.windows.contains_key(&hwnd) {
            continue;
        }
        visited += 1;
        match crate::x86::process::callbacks::invoke(
            cpu,
            mem,
            api,
            state,
            log,
            sched,
            callback,
            &[hwnd, param],
            max_instr,
        )? {
            GuestCallOutcome::Returned(value) => {
                returned = value;
                if value == 0 {
                    break;
                }
            }
            outcome => {
                return Ok((
                    outcome,
                    "window enumeration callback transferred control".into(),
                ))
            }
        }
    }
    Ok((
        GuestCallOutcome::Returned(returned),
        format!("enumerated {visited} windows"),
    ))
}
