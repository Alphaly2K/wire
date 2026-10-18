/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; see COPYING.LIB. Adapted from the Wine 10.0
 * window-text message flow previously in user32_common.rs.
 */
//! USER owns state transitions; this coordinator executes guest notifications.
use crate::{
    api::{ApiLogger, ApiState, ApiTable, Args},
    cpu::{Cpu, GuestCallOutcome},
    mem::GuestMem,
    thread::Sched,
    win32::catalog::WindowStateApi as S,
};

struct Change {
    value: u32,
    messages: [[u32; 4]; 2],
    count: usize,
}
impl super::User32 {
    fn change_owner(&mut self, id: S, hwnd: u32) -> Result<Change, u32> {
        let h = if id == S::ReleaseCapture { 0 } else { hwnd };
        if h != 0 && !self.windows.contains_key(&h) {
            return Err(1400);
        }
        let old = match id {
            S::SetFocus => self.focus,
            S::SetCapture | S::ReleaseCapture => self.capture,
            _ => self.active,
        };
        match id {
            S::SetFocus => self.focus = h,
            S::SetCapture | S::ReleaseCapture => self.capture = h,
            _ => {
                self.active = h;
                self.foreground = h;
            }
        }
        let messages = if old == h {
            [[0; 4]; 2]
        } else {
            match id {
                S::SetFocus => [[old, 8, h, 0], [h, 7, old, 0]],
                S::SetCapture | S::ReleaseCapture => [[old, 0x215, 0, h], [0; 4]],
                _ => [[old, 6, 0, h], [h, 6, 1, old]],
            }
        };
        Ok(Change {
            value: if matches!(id, S::ReleaseCapture | S::SetForeground) {
                1
            } else {
                old
            },
            messages,
            count: if old == h {
                0
            } else if matches!(id, S::SetCapture | S::ReleaseCapture) {
                1
            } else {
                2
            },
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn call(
    id: S,
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<(GuestCallOutcome, String), String> {
    let value = |n, note: &str| Ok((GuestCallOutcome::Returned(n), note.to_owned()));
    let text_message = match id {
        S::SetTextA | S::SetTextW => Some([a[0], 0xc, 0, a[1]]),
        S::TextA | S::TextW => Some([a[0], 0xd, a[2], a[1]]),
        S::TextLengthA | S::TextLengthW => Some([a[0], 0xe, 0, 0]),
        _ => None,
    };
    if let Some(message) = text_message {
        let error = match state.user32.windows.get(&a[0]).filter(|_| a[0] != 0) {
            None => Some(1400),
            Some(w) if w.tid != sched.current_tid() => Some(120),
            _ => None,
        };
        if let Some(error) = error {
            state.last_error = error;
            return value(0, &format!("USER32 error {error}"));
        }
        let procedure = state.user32.windows[&a[0]].wndproc;
        let outcome = super::invoke(
            cpu, mem, api, state, log, sched, procedure, &message, max_instr,
        )?;
        return Ok((outcome, "window/control message callback".into()));
    }
    match id {
        S::Focus => return value(state.user32.focus, "focus"),
        S::Active => return value(state.user32.active, "active window"),
        S::Foreground => return value(state.user32.foreground, "foreground window"),
        S::Capture => return value(state.user32.capture, "capture"),
        S::LastPopup => return value(a[0], "logical owner popup subset"),
        _ => {}
    }
    let change = match state.user32.change_owner(id, a[0]) {
        Ok(change) => change,
        Err(error) => {
            state.last_error = error;
            return value(0, &format!("USER32 error {error}"));
        }
    };
    for message in change.messages.into_iter().take(change.count) {
        // Resolve each procedure after the preceding callback, which can
        // destroy a window or replace its WNDPROC. State is already committed.
        if let Some(procedure) = state.user32.windows.get(&message[0]).map(|w| w.wndproc) {
            let outcome = super::invoke(
                cpu, mem, api, state, log, sched, procedure, &message, max_instr,
            )?;
            if !matches!(outcome, GuestCallOutcome::Returned(_)) {
                return Ok((outcome, "window state callback transferred".into()));
            }
        }
    }
    value(change.value, "logical input ownership")
}
