/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; see COPYING.LIB. Adapted from the Wine 10.0
 * window-text message flow previously in user32_common.rs.
 */
//! USER prepares window state and notifications without running the guest CPU.
use crate::{api::Args, win32::catalog::WindowStateApi as S};

pub(crate) enum WindowStateRequest {
    Value(u32, String),
    Text {
        procedure: u32,
        message: [u32; 4],
    },
    Notifications {
        value: u32,
        messages: [[u32; 4]; 2],
        count: usize,
    },
}

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

impl super::User32 {
    pub(crate) fn prepare_window_state(
        &mut self,
        id: S,
        a: &Args,
        tid: u32,
        last_error: &mut u32,
    ) -> WindowStateRequest {
        let value = |n, note: &str| WindowStateRequest::Value(n, note.to_owned());
        let text_message = match id {
            S::SetTextA | S::SetTextW => Some([a[0], 0xc, 0, a[1]]),
            S::TextA | S::TextW => Some([a[0], 0xd, a[2], a[1]]),
            S::TextLengthA | S::TextLengthW => Some([a[0], 0xe, 0, 0]),
            _ => None,
        };
        if let Some(message) = text_message {
            let error = match self.windows.get(&a[0]).filter(|_| a[0] != 0) {
                None => Some(1400),
                Some(w) if w.tid != tid => Some(120),
                _ => None,
            };
            if let Some(error) = error {
                *last_error = error;
                return value(0, &format!("USER32 error {error}"));
            }
            let procedure = self.windows[&a[0]].wndproc;
            return WindowStateRequest::Text { procedure, message };
        }
        match id {
            S::Focus => return value(self.focus, "focus"),
            S::Active => return value(self.active, "active window"),
            S::Foreground => return value(self.foreground, "foreground window"),
            S::Capture => return value(self.capture, "capture"),
            S::LastPopup => return value(a[0], "logical owner popup subset"),
            _ => {}
        }
        let change = match self.change_owner(id, a[0]) {
            Ok(change) => change,
            Err(error) => {
                *last_error = error;
                return value(0, &format!("USER32 error {error}"));
            }
        };
        WindowStateRequest::Notifications {
            value: change.value,
            messages: change.messages,
            count: change.count,
        }
    }
}
