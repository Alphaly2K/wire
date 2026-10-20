/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; see COPYING.LIB. Window/control flow previously
 * in user32_common.rs is adapted from Wine 10.0.
 */
//! USER owns lifecycle, send-queue and default-procedure state preparation.
use super::{SentMessage, User32, Window};
use crate::{
    api::Args,
    graphics::Graphics,
    mem::GuestMem,
    thread::{Sched, ThreadState, WaitTarget, Wake},
    win32::{catalog::WindowControlApi as W, x86::scratch::Scratch32},
};
pub(crate) enum CreationRequest {
    Value(u32, String),
    Window {
        hwnd: u32,
        create: u32,
        name: String,
    },
}
pub(crate) enum ControlRequest {
    Value(u32, String),
    Send { procedure: u32 },
    Wait { message: u32, note: &'static str },
    Show { old: u32, procedure: Option<u32> },
    Destroy,
}
pub(crate) enum DefaultRequest {
    Value(u32, String),
    Destroy,
    Position,
    Paint,
}
impl User32 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_creation(
        &mut self,
        a: &Args,
        wide: bool,
        mem: &mut GuestMem,
        scratch: &mut Scratch32,
        tid: u32,
        last_error: &mut u32,
    ) -> Result<CreationRequest, String> {
        let value = |value, note: String| Ok(CreationRequest::Value(value, note));
        let Some(atom) = self.find_class(mem, a[1], a[10], wide)? else {
            *last_error = 1407;
            return value(0, "window class not registered".into());
        };
        if a[8] != 0 && a[8] != 0xffff_fffd && !self.windows.contains_key(&a[8]) {
            *last_error = 1400;
            return value(0, "invalid parent window".into());
        }
        let class = self.classes.get(&atom).unwrap().clone();
        let hwnd = self.next_window;
        self.next_window = hwnd.checked_add(4).ok_or("window handle exhaustion")?;
        let rect = [a[4], a[5], a[6], a[7]].map(|n| if n == 0x8000_0000 { 0 } else { n as i32 });
        let window = Window {
            class: atom,
            wndproc: class.fields[1],
            instance: a[10],
            parent: a[8],
            menu: a[9],
            style: a[3],
            ex_style: a[0],
            rect,
            user_data: 0,
            extra: vec![0; class.fields[3] as usize],
            wide: class.wide,
            tid,
            text: crate::menu::read_text(mem, a[2], wide)
                .map_err(|e| format!("window title error {e}"))?
                .unwrap_or_default(),
            destroying: false,
            update: None,
            erase: false,
            props: Default::default(),
        };
        let create = scratch.alloc(mem, 48, 4, last_error);
        let mut fields = [
            a[11], a[10], a[9], a[8], a[7], a[6], a[5], a[4], a[3], a[2], a[1], a[0],
        ];
        if wide != class.wide {
            fields[10] = if a[1] <= 0xffff {
                a[1]
            } else {
                class.name_ptrs[class.wide as usize]
            };
            if a[2] != 0 {
                let title = crate::file_api::read_name(mem, a[2], wide)
                    .map_err(|e| format!("window title: {e}"))?;
                fields[9] = super::classes::persistent_strings(mem, scratch, last_error, &title)?
                    [class.wide as usize];
            }
        }
        for (i, field) in fields.into_iter().enumerate() {
            mem.write_u32(create + i as u32 * 4, field)
                .map_err(|e| e.to_string())?;
        }
        self.windows.insert(hwnd, window);
        Ok(CreationRequest::Window {
            hwnd,
            create,
            name: class.name,
        })
    }
    pub(crate) fn prepare_control(
        &mut self,
        id: W,
        a: &Args,
        sched: &mut Sched,
        last_error: &mut u32,
    ) -> Result<ControlRequest, String> {
        let value = |value, note: String| Ok(ControlRequest::Value(value, note));
        if matches!(id, W::SendA | W::SendW) {
            let sender = sched.current_tid();
            if let Some((&id, msg)) = self
                .sent
                .iter()
                .find(|(_, m)| m.sender == sender && m.result.is_some() && m.args == a[..4])
            {
                let result = msg.result.unwrap();
                self.sent.remove(&id);
                sched.take_wake();
                return value(
                    result,
                    "cross-thread WndProc completed or owner was removed".into(),
                );
            }
        }
        let Some(window) = self.windows.get_mut(&a[0]) else {
            *last_error = 1400;
            return value(0, "invalid HWND".into());
        };
        match id {
            W::SendA | W::SendW => {
                if window.tid != sched.current_tid() {
                    let owner = window.tid;
                    let sender = sched.current_tid();
                    if let Some((&id, request)) = self.sent.iter().find(|(_, m)| m.sender == sender)
                    {
                        if let Some(result) = request.result {
                            self.sent.remove(&id);
                            sched.take_wake();
                            return value(
                                result,
                                "cross-thread WndProc completed on its owner".into(),
                            );
                        }
                        return Ok(ControlRequest::Wait {
                            message: id,
                            note: "waiting for WndProc owner",
                        });
                    }
                    if self.sent.len() >= 256 {
                        *last_error = 8;
                        return value(0, "sent message limit".into());
                    }
                    let Some(receiver) = sched
                        .threads
                        .iter()
                        .position(|t| t.tid == owner && !matches!(t.state, ThreadState::Exited(_)))
                    else {
                        *last_error = 1400;
                        return value(0, "WndProc owner exited".into());
                    };
                    let id = self.next_sent;
                    self.next_sent = self
                        .next_sent
                        .checked_add(1)
                        .ok_or("sent message ID exhausted")?;
                    self.sent.insert(
                        id,
                        SentMessage {
                            sender,
                            receiver: owner,
                            args: a[..4].try_into().unwrap(),
                            processing: false,
                            result: None,
                        },
                    );
                    if matches!(
                        sched.threads[receiver].state,
                        ThreadState::Waiting {
                            target: WaitTarget::Messages(_) | WaitTarget::SentMessage(_),
                            ..
                        }
                    ) || matches!(sched.threads[receiver].state,ThreadState::Waiting {
                        target:WaitTarget::MessageObjects(_,_,mask),..} if mask&0x40!=0)
                    {
                        sched.wake_thread(receiver, Wake::Retest);
                    }
                    return Ok(ControlRequest::Wait {
                        message: id,
                        note: "queued synchronous WndProc for owner thread",
                    });
                }
                Ok(ControlRequest::Send {
                    procedure: window.wndproc,
                })
            }
            W::Show => {
                let old = (window.style & 0x1000_0000 != 0) as u32;
                if a[1] == 0 {
                    window.style &= !0x1000_0000;
                } else {
                    window.style |= 0x1000_0000;
                    let [width, height] = window.client_size();
                    window.update = Some([0, 0, width, height]);
                    window.erase = true;
                }
                let changed = old != (a[1] != 0) as u32;
                Ok(ControlRequest::Show {
                    old,
                    procedure: changed.then_some(window.wndproc),
                })
            }
            W::Destroy => {
                if window.tid != sched.current_tid() {
                    *last_error = 5;
                    return value(0, "window belongs to another thread".into());
                }
                if window.destroying {
                    return value(0, "window already being destroyed".into());
                }
                window.destroying = true;
                Ok(ControlRequest::Destroy)
            }
            _ => unreachable!("window lifecycle or synchronous send"),
        }
    }
    pub(crate) fn prepare_default(
        &mut self,
        wide: bool,
        a: &Args,
        mem: &GuestMem,
        graphics: &mut Graphics,
    ) -> Result<DefaultRequest, String> {
        let value = |value, note: String| Ok(DefaultRequest::Value(value, note));
        if a[1] == 0x10 {
            return Ok(DefaultRequest::Destroy);
        }
        if let Some(ret) = crate::user32_common::default_text(wide, a, mem, self)? {
            return value(ret, "default window text".into());
        }
        if a[1] == 0x47 {
            return Ok(DefaultRequest::Position);
        }
        if a[1] == 0x83 {
            if let Some(w) = self.windows.get(&a[0]) {
                let f = w.frame();
                let mut bytes = [0; 16];
                mem.read_bytes(a[3], &mut bytes)
                    .map_err(|e| e.to_string())?;
                let mut r: [i32; 4] = std::array::from_fn(|i| {
                    i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
                for i in 0..4 {
                    r[i] = if i < 2 {
                        r[i].saturating_add(f[i])
                    } else {
                        r[i].saturating_sub(f[i])
                    };
                }
                r[2] = r[2].max(r[0]);
                r[3] = r[3].max(r[1]);
                mem.write_bytes(
                    a[3],
                    &r.into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>(),
                )
                .map_err(|e| e.to_string())?;
            }
            return value(0, "default nonclient geometry".into());
        }
        if a[1] == 0xf {
            return Ok(DefaultRequest::Paint);
        }
        if a[1] == 0x14 {
            let brush = self
                .windows
                .get(&a[0])
                .and_then(|w| self.classes.get(&w.class))
                .map_or(0, |c| c.fields[7]);
            if (1..=31).contains(&brush) {
                let color = match brush - 1 {
                    5 => 0xffffff,
                    8 | 18 => 0,
                    15 => 0xf0f0f0,
                    _ => 0xc0c0c0,
                };
                let result = graphics.fill_color(mem, a[2], color).is_ok() as u32;
                return value(result, "default system-color background".into());
            }
            return value(0, "no supported class background brush".into());
        }
        // Default acceptance of WM_NCCREATE; the implemented default subset
        // has no application action for WM_CREATE/DESTROY or unknown messages.
        value((a[1] == 0x81) as u32, "default WndProc subset".into())
    }
}
