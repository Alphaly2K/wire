/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 *
 * This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public License
 * as published by the Free Software Foundation; either version 2.1
 * of the License, or (at your option) any later version.
 * This library is distributed without any warranty; see COPYING.LIB.
 */
//! USER prepares control messages and snapshots without executing callbacks.
use crate::{
    api::Args,
    mem::GuestMem,
    win32::{catalog::DialogApi as D, x86::scratch::Scratch32},
};
use std::collections::BTreeSet;

pub(crate) enum DialogRequest {
    Value(u32, String),
    Message {
        procedure: u32,
        args: [u32; 4],
        output: Option<u32>,
    },
    Enumeration {
        windows: Vec<u32>,
        callback: u32,
        param: u32,
        children: bool,
    },
}
type ResultValue = Result<DialogRequest, String>;
fn value(n: u32, note: &str) -> ResultValue {
    Ok(DialogRequest::Value(n, note.into()))
}
fn failure(last_error: &mut u32, error: u32) -> ResultValue {
    *last_error = error;
    value(0, &format!("USER32 error {error}"))
}
fn child(user: &super::User32, parent: u32, id: u32) -> Option<u32> {
    user.windows.iter().find_map(|(&h, w)| {
        (w.parent == parent && w.style & 0x40000000 != 0 && w.menu == id).then_some(h)
    })
}

impl super::User32 {
    pub(crate) fn prepare_dialog(
        &self,
        id: D,
        a: &Args,
        mem: &mut GuestMem,
        scratch: &mut Scratch32,
        tid: u32,
        last_error: &mut u32,
    ) -> ResultValue {
        if id.unavailable() {
            return failure(last_error, 120);
        }
        if matches!(id, D::Windows | D::ThreadWindows) {
            return self.prepare_window_enumeration(id, a, last_error);
        }
        // Dialog/control wrappers use the same application WndProc boundary
        // as SendMessage. Nonlocal transfers are propagated to the caller.
        if id.message() {
            let hwnd = child(self, a[0], a[1]).unwrap_or(0);
            if hwnd == 0 || !self.windows.contains_key(&hwnd) {
                return failure(last_error, 1400);
            }
            let w = &self.windows[&hwnd];
            if w.tid != tid {
                return failure(last_error, 120);
            }
            let proc = w.wndproc;
            let mut output = None;
            let (message, wp, lp) = if id.send() {
                (a[2], a[3], a[4])
            } else if id == D::GetInteger {
                if a[2] != 0 {
                    mem.write_u32(a[2], 0).map_err(|e| e.to_string())?;
                }
                let p = scratch.alloc(mem, 60, 2, last_error);
                if p == 0 {
                    return failure(last_error, 8);
                }
                output = Some(p);
                (0xd, 30, p)
            } else if id == D::SetInteger {
                let text = if a[3] != 0 {
                    (a[2] as i32).to_string()
                } else {
                    a[2].to_string()
                };
                let p = scratch.alloc(mem, (text.len() as u32 + 1) * 2, 2, last_error);
                if p == 0 {
                    return failure(last_error, 8);
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
            return Ok(DialogRequest::Message {
                procedure: proc,
                args: [hwnd, message, wp, lp],
                output,
            });
        }
        match id {
            D::Item => value(child(self, a[0], a[1]).unwrap_or(0), "child control lookup"),
            D::ControlId => value(self.windows.get(&a[0]).map_or(0, |w| w.menu), "control ID"),
            D::Children => {
                let mut seen = BTreeSet::new();
                let mut pending = vec![a[0]];
                let mut children = Vec::new();
                while let Some(parent) = pending.pop() {
                    if !seen.insert(parent) {
                        continue;
                    }
                    for (&h, w) in &self.windows {
                        if w.parent == parent && w.style & 0x40000000 != 0 {
                            children.push(h);
                            pending.push(h);
                        }
                    }
                }
                Ok(DialogRequest::Enumeration {
                    windows: children,
                    callback: a[1],
                    param: a[2],
                    children: true,
                })
            }
            _ => unreachable!("other dialog IDs were handled before child lookup"),
        }
    }

    fn prepare_window_enumeration(&self, id: D, a: &Args, last_error: &mut u32) -> ResultValue {
        let (tid, callback, param) = if id == D::ThreadWindows {
            (a[0], a[1], a[2])
        } else {
            (0, a[0], a[1])
        };
        if callback == 0 {
            *last_error = 87;
            return value(0, "null enumeration callback");
        }
        let windows: Vec<_> = self
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
        Ok(DialogRequest::Enumeration {
            windows,
            callback,
            param,
            children: false,
        })
    }
}

pub(crate) fn dialog_integer(a: &Args, mem: &GuestMem, p: u32, count: u32) -> Result<u32, String> {
    let text = mem
        .read_wstr(p, count.min(29) as usize)
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
    Ok(n.unwrap_or(0))
}
