/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; see COPYING.LIB. Window/control flow previously
 * in user32_common.rs is adapted from Wine 10.0.
 */
//! Window geometry and update preparation without guest execution.
use super::User32;
use crate::{
    api::Args,
    mem::GuestMem,
    user32_common::{words, write_words},
    win32::{catalog::WindowControlApi as W, x86::scratch::Scratch32},
};
use std::collections::BTreeSet;

pub(crate) enum WindowRequest {
    Value(u32, String),
    Notify {
        procedure: u32,
        args: [u32; 4],
        value: u32,
        note: &'static str,
        transfer: &'static str,
    },
    Position(PositionChange),
}
pub(crate) struct PositionChange {
    pub hwnd: u32,
    pub procedure: u32,
    pub buffer: u32,
    pub notify: bool,
    old_rect: [i32; 4],
}
type ResultValue = Result<WindowRequest, String>;
fn value(n: u32, note: &str) -> ResultValue {
    Ok(WindowRequest::Value(n, note.into()))
}
fn failure(last_error: &mut u32, error: u32) -> ResultValue {
    *last_error = error;
    value(0, &format!("USER32 error {error}"))
}
fn pack(x: i32, y: i32) -> u32 {
    (x as u32 & 0xffff) | ((y as u32) << 16)
}
pub(crate) struct PositionNotifications {
    pub movement: Option<(u32, [u32; 4])>,
    hwnd: u32,
    flags: u32,
}
impl User32 {
    pub(crate) fn prepare_geometry(
        &mut self,
        id: W,
        a: &Args,
        mem: &mut GuestMem,
        scratch: &mut Scratch32,
        last_error: &mut u32,
    ) -> ResultValue {
        match id {
            W::Enable => {
                let Some(w) = self.windows.get_mut(&a[0]) else {
                    return failure(last_error, 1400);
                };
                let disabled = w.style & 0x08000000 != 0;
                if a[1] != 0 {
                    w.style &= !0x08000000;
                } else {
                    w.style |= 0x08000000;
                }
                let proc = w.wndproc;
                if disabled == (a[1] != 0) {
                    return Ok(WindowRequest::Notify {
                        procedure: proc,
                        args: [a[0], 0xa, (a[1] != 0) as u32, 0],
                        value: disabled as u32,
                        note: "window enabled state",
                        transfer: "WM_ENABLE transferred",
                    });
                }
                value(disabled as u32, "window enabled state")
            }
            W::Parent => {
                if a[1] != 0 && !self.windows.contains_key(&a[1]) {
                    return failure(last_error, 1400);
                }
                let mut h = a[1];
                let mut seen = BTreeSet::new();
                while h != 0 {
                    if h == a[0] || !seen.insert(h) {
                        return failure(last_error, 87);
                    }
                    h = self.windows.get(&h).map_or(0, |w| w.parent);
                }
                let screen = self.client_origin(a[0])?;
                let parent = self.client_origin(a[1])?;
                let Some(w) = self.windows.get_mut(&a[0]) else {
                    return failure(last_error, 1400);
                };
                let old = w.parent;
                w.parent = a[1];
                if w.style & 0x40000000 != 0 {
                    w.rect[0] = screen[0].wrapping_sub(parent[0]);
                    w.rect[1] = screen[1].wrapping_sub(parent[1]);
                }
                value(old, "parent changed; style retained")
            }
            W::Move | W::Position => {
                let Some(w) = self.windows.get(&a[0]) else {
                    return failure(last_error, 1400);
                };
                let proc = w.wndproc;
                let old_rect = w.rect;
                let flags = if id == W::Move { 0x14 } else { a[6] };
                let offset = if id == W::Move { 1 } else { 2 };
                let p = scratch.alloc(mem, 28, 4, last_error);
                if p == 0 {
                    return failure(last_error, 8);
                }
                write_words(
                    mem,
                    p,
                    &[
                        a[0] as i32,
                        if id == W::Move { 0 } else { a[1] as i32 },
                        a[offset] as i32,
                        a[offset + 1] as i32,
                        a[offset + 2] as i32,
                        a[offset + 3] as i32,
                        flags as i32,
                    ],
                )?;
                Ok(WindowRequest::Position(PositionChange {
                    hwnd: a[0],
                    procedure: proc,
                    buffer: p,
                    old_rect,
                    notify: flags & 0x400 == 0,
                }))
            }
            _ => unreachable!("window geometry control"),
        }
    }
    pub(crate) fn commit_position(
        &mut self,
        change: PositionChange,
        mem: &GuestMem,
        last_error: &mut u32,
    ) -> ResultValue {
        let PositionChange {
            hwnd,
            procedure,
            buffer: p,
            old_rect,
            ..
        } = change;
        let pos = words(mem, p, 7)?;
        let mut flags = pos[6] as u32;
        let Some(w) = self.windows.get_mut(&hwnd) else {
            return failure(last_error, 1400);
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

        Ok(WindowRequest::Notify {
            procedure,
            args: [hwnd, 0x47, 0, p],
            value: 1,
            note: "logical window position",
            transfer: "WM_WINDOWPOSCHANGED transferred",
        })
    }
    pub(crate) fn prepare_update(&self, a: &Args, last_error: &mut u32) -> ResultValue {
        if !self.windows.contains_key(&a[0]) {
            *last_error = 1400;
            return value(0, "window update/paint state");
        }
        let w = &self.windows[&a[0]];
        if w.update.is_none() {
            return value(1, "window update/paint state");
        }

        Ok(WindowRequest::Notify {
            procedure: w.wndproc,
            args: [a[0], 0xf, 0, 0],
            value: 1,
            note: "window update/paint state",
            transfer: "WM_PAINT transferred",
        })
    }
    // Adapted from Hangover Wine win32u/defwnd.c handle_window_pos_changed
    // (LGPL-2.1-or-later): Alexandre Julliard, Alex Korobka, Jacek Caban.
    pub(crate) fn prepare_windowpos(
        &self,
        a: &Args,
        mem: &GuestMem,
    ) -> Result<Option<PositionNotifications>, String> {
        let flags = mem
            .read_u32(a[3].checked_add(24).ok_or("WINDOWPOS overflow")?)
            .map_err(|e| e.to_string())?;
        let Some(w) = self.windows.get(&a[0]) else {
            return Ok(None);
        };
        let rect = w.rect;
        let f = w.frame();
        let movement = if flags & 0x1000 == 0 {
            Some((
                w.wndproc,
                [a[0], 3, 0, pack(rect[0] + f[0], rect[1] + f[1])],
            ))
        } else {
            None
        };
        Ok(Some(PositionNotifications {
            movement,
            hwnd: a[0],
            flags,
        }))
    }
    pub(crate) fn size_notification(
        &self,
        position: &PositionNotifications,
    ) -> Option<(u32, [u32; 4])> {
        let PositionNotifications { hwnd, flags, .. } = *position;
        if (flags & 0x800 == 0 || flags & 0x8000 != 0) && self.windows.contains_key(&hwnd) {
            let w = &self.windows[&hwnd];
            let (wp, lp) = if w.style & 0x20000000 != 0 {
                (1, 0)
            } else {
                (
                    (w.style & 0x01000000 != 0) as u32 * 2,
                    pack(w.client_size()[0], w.client_size()[1]),
                )
            };
            Some((w.wndproc, [hwnd, 5, wp, lp]))
        } else {
            None
        }
    }
}
