/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; LGPL 2.1 selected for wire's combined work.
 * PE32 lifecycle rules retain the existing Wine 10.0 window-control derivation.
 */
//! WNDPROC calls use the same guest callback stack as exceptions and lookups.
use super::{
    callbacks::{CallFrame, Callback},
    unwind, *,
};
use crate::win32::{
    call::Call,
    catalog::{WindowControlApi as W, WindowStateApi as S},
    user::{self, classes, Window},
    ServiceError,
};

#[path = "window_dialog.rs"]
mod dialog;
#[path = "window_paint.rs"]
mod painting;
#[path = "window_position.rs"]
mod position;
#[path = "window_relationship.rs"]
mod relationship;

#[derive(Clone)]
enum Operation {
    Message,
    TimedMessage {
        window: u64,
        output: u64,
        error_on_exit: bool,
        completed: Option<u64>,
    },
    Timer,
    Paint(painting::Paint),
    Position(position::Position),
    Enumeration(dialog::Enumeration),
    Integer(dialog::Integer),
    Notifications {
        messages: Vec<[u64; 4]>,
        next: usize,
        result: u64,
    },
    Create {
        window: u64,
        stage: u8,
    },
    Destroy {
        window: u64,
        stage: u8,
        result: u64,
    },
    Show {
        window: u64,
        previous: u64,
        stage: u8,
    },
}
#[derive(Clone)]
pub(crate) struct Frame {
    pub(super) saved: Box<State>,
    pub(super) loader_depth: usize,
    call: Call,
    operation: Operation,
    block: u64,
    return_sp: u64,
}
impl Frame {
    pub(super) fn unwind_bridge(&self) -> unwind::Bridge {
        let mut state = self.saved.clone();
        self.call.clone().complete(&mut state, 0);
        unwind::Bridge {
            sp: self.return_sp,
            caller: Some((state, true)),
        }
    }
    pub(super) fn abandoned_dc(&self) -> Option<u64> {
        if let Operation::Paint(p) = &self.operation {
            Some(p.dc)
        } else {
            None
        }
    }
    pub(super) fn abandoned_window(&self) -> Option<u64> {
        match self.operation {
            Operation::Create { window, .. } | Operation::Destroy { window, .. } => Some(window),
            _ => None,
        }
    }
}
impl Loaded {
    pub(crate) fn window_state_call(
        &mut self,
        id: S,
        mut call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), ServiceError> {
        let a = call.arguments;
        let tid = self.threads.active;
        if id == S::AllowForeground {
            let value = user::error(memory, state, 50)?;
            call.complete(state, value);
            return Ok(());
        }
        let value = match id {
            S::Focus => Some(self.user.input.get(&tid).map_or(0, |s| s.focus)),
            S::Active => Some(self.user.input.get(&tid).map_or(0, |s| s.active)),
            S::Capture => Some(self.user.input.get(&tid).map_or(0, |s| s.capture)),
            S::Foreground => Some(self.user.foreground),
            _ => None,
        };
        if let Some(value) = value {
            call.complete(state, value);
            return Ok(());
        }
        let target = if id == S::ReleaseCapture { 0 } else { a[0] };
        let text = matches!(
            id,
            S::SetTextA | S::SetTextW | S::TextA | S::TextW | S::TextLengthA | S::TextLengthW
        );
        if target != 0 {
            let Some(w) = self.user.windows.get(&target) else {
                let v = user::error(memory, state, 1400)?;
                call.complete(state, v);
                return Ok(());
            };
            if w.tid != u64::from(tid) {
                let v = user::error(memory, state, 50)?;
                call.complete(state, v);
                return Ok(());
            }
        } else if text || matches!(id, S::SetForeground | S::LastPopup) {
            let v = user::error(memory, state, 1400)?;
            call.complete(state, v);
            return Ok(());
        }
        if id == S::LastPopup {
            call.complete(state, target);
            return Ok(());
        }
        if text {
            if matches!(id, S::TextA | S::TextW) && (a[2] as u32 as i32) <= 0 {
                call.complete(state, 0);
                return Ok(());
            }
            call.arguments[..4].copy_from_slice(&match id {
                S::SetTextA | S::SetTextW => [target, 0xc, 0, a[1]],
                S::TextA | S::TextW => [target, 0xd, a[2], a[1]],
                _ => [target, 0xe, 0, 0],
            });
            return self.window_call(
                if id.wide() { W::SendW } else { W::SendA },
                call,
                state,
                memory,
                services,
            );
        }
        if id == S::SetForeground
            && self.user.foreground != 0
            && self
                .user
                .windows
                .get(&self.user.foreground)
                .is_some_and(|w| w.tid != u64::from(tid))
        {
            let v = user::error(memory, state, 50)?;
            call.complete(state, v);
            return Ok(());
        }
        let previous = self.user.input.get(&tid).map_or(0, |s| match id {
            S::SetFocus => s.focus,
            S::SetCapture | S::ReleaseCapture => s.capture,
            _ => s.active,
        });
        let result = if matches!(id, S::SetForeground | S::ReleaseCapture) {
            1
        } else {
            previous
        };
        let messages = if previous == target {
            Vec::new()
        } else {
            match id {
                S::SetFocus => vec![[previous, 8, target, 0], [target, 7, previous, 0]],
                S::SetCapture | S::ReleaseCapture => vec![[previous, 0x215, 0, target]],
                _ => vec![[previous, 6, 0, target], [target, 6, 1, previous]],
            }
        };
        self.notifications(call, state, memory, services, messages, result)?;
        let input = self.user.input.entry(tid).or_default();
        match id {
            S::SetFocus => input.focus = target,
            S::SetCapture | S::ReleaseCapture => input.capture = target,
            _ => input.active = target,
        }
        if id == S::SetForeground {
            self.user.foreground = target;
        }
        Ok(())
    }
    fn notifications(
        &mut self,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
        messages: Vec<[u64; 4]>,
        result: u64,
    ) -> Result<(), ServiceError> {
        let Some(index) = messages
            .iter()
            .position(|a| self.user.windows.contains_key(&a[0]))
        else {
            call.complete(state, result);
            return Ok(());
        };
        if self.threads.current().callbacks.len() >= 32 {
            return Err(ServiceError::Unavailable(
                "USER callback nesting capacity exceeded",
            ));
        }
        let args = messages[index];
        let target = self.user.windows[&args[0]].procedure;
        user::procedure(memory, services, target)?;
        let block = state.regs[4] & !15;
        let entry = CallFrame::new(memory.mode(), block, &args)?;
        let frame = Frame {
            saved: Box::new(state.clone()),
            loader_depth: self.callback_depth(),
            call,
            operation: Operation::Notifications {
                messages,
                next: index + 1,
                result,
            },
            block,
            return_sp: entry.sp + if memory.mode() == Mode::X86 { 20 } else { 8 },
        };
        entry.enter(target, state, memory)?;
        self.threads
            .current_mut()
            .callbacks
            .push(Callback::Window(Box::new(frame)));
        Ok(())
    }
    pub(crate) fn window_call(
        &mut self,
        id: W,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), ServiceError> {
        if id.timeout() {
            let a = &call.arguments;
            let error = if a[4] as u32 & !0x2b != 0 {
                Some(87)
            } else if a[0] == 0xffff {
                Some(50)
            } else {
                None
            };
            if let Some(code) = error {
                let value = user::error(memory, state, code)?;
                call.complete(state, value);
                return Ok(());
            }
        }
        if id == W::Update {
            let window = call.arguments[0];
            let Some(w) = self.user.windows.get(&window) else {
                let v = user::error(memory, state, 1400)?;
                call.complete(state, v);
                return Ok(());
            };
            if w.tid != u64::from(self.threads.active) {
                let v = user::error(memory, state, 50)?;
                call.complete(state, v);
                return Ok(());
            }
            let messages = if self
                .user
                .paints
                .get(&window)
                .is_some_and(|p| p.area.is_some())
            {
                vec![[window, 0xf, 0, 0]]
            } else {
                Vec::new()
            };
            return self.notifications(call, state, memory, services, messages, 1);
        }
        if matches!(id, W::Move | W::Position) {
            return self.position_call(id, call, state, memory, services);
        }
        if matches!(id, W::Parent | W::Enable) {
            return self.window_relationship(id, call, state, memory, services);
        }
        self.procedure_call(id, call, state, memory, services, false)
    }
    pub(crate) fn timer_call(
        &mut self,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), ServiceError> {
        self.procedure_call(W::CallW, call, state, memory, services, true)
    }
    fn procedure_call(
        &mut self,
        id: W,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
        timer: bool,
    ) -> Result<(), ServiceError> {
        let mode = memory.mode();
        let a = &call.arguments;
        if id.default_procedure() && a[1] == 0xf {
            return self.default_paint(call, state, memory, services);
        }
        if id.default_procedure() && a[1] == 0x47 {
            return self.default_position(call, state, memory, services);
        }
        if id.default_procedure() && a[1] != 0x10 {
            let value = self
                .user
                .default_call(id, a, state, memory, self.threads.active)?;
            call.complete(state, value);
            return Ok(());
        }
        let closing = id.default_procedure();
        let id = if closing { W::Destroy } else { id };
        if self.threads.current().callbacks.len() >= 32 {
            return Err(ServiceError::Unavailable(
                "USER callback nesting capacity exceeded",
            ));
        }
        let creating = matches!(id, W::CreateA | W::CreateW);
        let mut block = (state.regs[4] & !15)
            .checked_sub(if creating { 160 } else { 0 })
            .ok_or(ServiceError::Unavailable("WNDPROC stack underflow"))?;
        let mut frame = CallFrame::new(mode, block, &[0; 4])?;
        memory.check(frame.sp, frame.bytes.len(), WRITE)?;
        let (target, args, operation) = if creating {
            memory.check(block, 160, WRITE)?;
            let text = if a[1] > 65535 {
                Some(if mode == Mode::X86 {
                    crate::win32::class_services::window_class_name(memory, a[1], id.wide())?
                } else {
                    user::name(memory, a[1], id.wide())?
                })
            } else {
                None
            };
            let key = text
                .as_deref()
                .map_or(classes::Key::Id(a[1] as u32), classes::Key::Name);
            let Some((atom, class)) = self
                .user
                .classes
                .find(a[10], key, true)
                .map(|(atom, c)| (atom, c.clone()))
            else {
                let v = user::error(memory, state, 1407)?;
                call.complete(state, v);
                return Ok(());
            };
            if mode == Mode::X64 && a[9] != 0 && a[3] & 0x40000000 == 0 {
                let v = user::error(memory, state, 50)?;
                call.complete(state, v);
                return Ok(());
            }
            if a[8] != 0 && !user::hierarchy::message_parent(a[8]) {
                let Some(parent) = self.user.windows.get(&a[8]) else {
                    let value = user::error(memory, state, 1400)?;
                    call.complete(state, value);
                    return Ok(());
                };
                if parent.tid != 0 && parent.tid != u64::from(self.threads.active) {
                    let value = user::error(memory, state, 50)?;
                    call.complete(state, value);
                    return Ok(());
                }
            }
            if mode == Mode::X86
                && a[3] & 0x40000000 == 0
                && a[9] != 0
                && !self.user.menus.contains(a[9] as u32)
            {
                let v = user::error(memory, state, 1401)?;
                call.complete(state, v);
                return Ok(());
            }
            user::procedure(memory, services, class.procedure)?;
            let title = if a[2] == 0 {
                String::new()
            } else {
                user::name(memory, a[2], id.wide())?
            };
            let mut dimensions: [i32; 4] = std::array::from_fn(|n| a[4 + n] as u32 as i32);
            for (n, v) in [100, 100, 800, 600].into_iter().enumerate() {
                if dimensions[n] == i32::MIN {
                    dimensions[n] = v
                }
            }
            if dimensions[2] <= 0
                || dimensions[3] <= 0
                || dimensions[2] > 8192
                || dimensions[3] > 8192
                || dimensions[0].checked_add(dimensions[2]).is_none()
                || dimensions[1].checked_add(dimensions[3]).is_none()
            {
                let v = user::error(memory, state, 87)?;
                call.complete(state, v);
                return Ok(());
            }
            if self.user.windows.len() >= 1024
                || self.user.next_window.checked_add(16).is_none_or(|h| {
                    h > if mode == Mode::X86 {
                        0x40020000
                    } else {
                        0x7ffd00640000
                    }
                })
            {
                let v = user::error(memory, state, 8)?;
                call.complete(state, v);
                return Ok(());
            }
            let r = [
                dimensions[0],
                dimensions[1],
                dimensions[0].wrapping_add(dimensions[2]),
                dimensions[1].wrapping_add(dimensions[3]),
            ];
            let wide = self
                .user
                .class_metadata
                .get(&atom)
                .map_or(class.wide, |metadata| metadata.wide);
            let cross = wide != id.wide();
            let encode = |text: &str| -> Result<Vec<u8>, u32> {
                let mut bytes = if wide {
                    text.encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                } else {
                    crate::codepage::encode(text, 932)?.0
                };
                bytes.resize(bytes.len() + if wide { 2 } else { 1 }, 0);
                Ok(bytes)
            };
            let translated = (|| -> Result<(Vec<u8>, Option<usize>), u32> {
                let mut bytes = if cross && a[2] != 0 {
                    encode(&title)?
                } else {
                    Vec::new()
                };
                let class_offset = if cross && mode == Mode::X64 && a[1] > 65535 {
                    let offset = bytes.len();
                    bytes.extend(encode(&class.name)?);
                    Some(offset)
                } else {
                    None
                };
                Ok((bytes, class_offset))
            })();
            let (translated, class_offset) = match translated {
                Ok(result) => result,
                Err(code) => {
                    let value = user::error(memory, state, code)?;
                    call.complete(state, value);
                    return Ok(());
                }
            };
            let translated_title = cross && a[2] != 0;
            if !translated.is_empty() {
                block = block
                    .checked_sub((translated.len() as u64).div_ceil(16) * 16)
                    .ok_or(ServiceError::Unavailable(
                        "WNDPROC translated text stack underflow",
                    ))?;
                frame = CallFrame::new(mode, block, &[0; 4])?;
                memory.check(frame.sp, frame.bytes.len(), WRITE)?;
                memory.check(block, 160 + translated.len(), WRITE)?;
            }
            let title_pointer = if translated_title { block + 160 } else { a[2] };
            let class_pointer = if let Some(offset) = class_offset {
                block + 160 + offset as u64
            } else if cross && a[1] > 65535 {
                self.user.class_metadata[&atom].name_ptrs[usize::from(wide)].into()
            } else {
                a[1]
            };
            let mut bytes = vec![0; 160 + translated.len()];
            bytes[160..].copy_from_slice(&translated);
            if mode == Mode::X86 {
                for (n, v) in [
                    a[11],
                    a[10],
                    a[9],
                    a[8],
                    dimensions[3] as u32 as u64,
                    dimensions[2] as u32 as u64,
                    dimensions[1] as u32 as u64,
                    dimensions[0] as u32 as u64,
                    a[3],
                    title_pointer,
                    class_pointer,
                    a[0],
                ]
                .into_iter()
                .enumerate()
                {
                    bytes[n * 4..n * 4 + 4].copy_from_slice(&(v as u32).to_le_bytes());
                }
            } else {
                for (at, v) in [
                    (0, a[11]),
                    (8, a[10]),
                    (16, a[9]),
                    (24, a[8]),
                    (56, title_pointer),
                    (64, class_pointer),
                ] {
                    bytes[at..at + 8].copy_from_slice(&v.to_le_bytes());
                }
                for (at, v) in [
                    (32, dimensions[3] as u32),
                    (36, dimensions[2] as u32),
                    (40, dimensions[1] as u32),
                    (44, dimensions[0] as u32),
                    (48, a[3] as u32),
                    (72, a[0] as u32),
                    (104, 1280),
                    (108, 720),
                    (120, 112),
                    (124, 27),
                    (128, 1280),
                    (132, 720),
                ] {
                    bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
            for (n, v) in r.into_iter().enumerate() {
                bytes[144 + n * 4..148 + n * 4].copy_from_slice(&v.to_le_bytes());
            }
            memory.write(block, &bytes)?;
            let window = self.user.next_window;
            self.user.next_window += 16;
            self.user.windows.insert(
                window,
                Window {
                    class: atom,
                    procedure: class.procedure,
                    instance: a[10],
                    parent: a[8],
                    menu: a[9],
                    style: a[3] as u32,
                    ex_style: a[0] as u32,
                    rect: r,
                    client: if mode == Mode::X86 {
                        let f = user::geometry::frame(
                            a[3] as u32,
                            a[0] as u32,
                            a[3] & 0x40000000 == 0 && a[9] != 0,
                            true,
                        );
                        let left = r[0].saturating_add(f[0]);
                        let top = r[1].saturating_add(f[1]);
                        [
                            left,
                            top,
                            r[2].saturating_sub(f[2]).max(left),
                            r[3].saturating_sub(f[3]).max(top),
                        ]
                    } else {
                        r
                    },
                    data: 0,
                    extra: vec![0; class.window_extra],
                    tid: u64::from(self.threads.active),
                    wide,
                    title,
                    ready: false,
                },
            );
            (
                class.procedure,
                [
                    window,
                    if mode == Mode::X86 { 0x81 } else { 0x24 },
                    0,
                    if mode == Mode::X86 { block } else { block + 96 },
                ],
                Operation::Create {
                    window,
                    stage: if mode == Mode::X86 { 1 } else { 0 },
                },
            )
        } else if matches!(id, W::CallA | W::CallW) {
            user::procedure(memory, services, a[0])?;
            (
                a[0],
                [a[1], a[2], a[3], a[4]],
                if timer {
                    Operation::Timer
                } else {
                    Operation::Message
                },
            )
        } else {
            let Some(w) = self.user.windows.get_mut(&a[0]) else {
                let v = user::error(memory, state, 1400)?;
                call.complete(state, v);
                return Ok(());
            };
            if w.tid != u64::from(self.threads.active) {
                let v = user::error(memory, state, if id == W::Destroy { 5 } else { 50 })?;
                call.complete(state, v);
                return Ok(());
            }
            if (matches!(id, W::SendA | W::SendW) || id.timeout())
                && w.wide != id.wide()
                && matches!(a[1], 0xc..=0xe)
            {
                let v = user::error(memory, state, 50)?;
                call.complete(state, v);
                return Ok(());
            }
            user::procedure(memory, services, w.procedure)?;
            if id == W::Destroy {
                if !self.user.destroying.insert(a[0]) {
                    call.complete(state, 0);
                    return Ok(());
                }
                (
                    w.procedure,
                    [a[0], 2, 0, 0],
                    Operation::Destroy {
                        window: a[0],
                        stage: 0,
                        result: if closing { 0 } else { 1 },
                    },
                )
            } else if id == W::Show {
                if mode == Mode::X64 && !matches!(a[1], 0 | 1 | 4 | 5 | 8) {
                    let v = user::error(memory, state, 50)?;
                    call.complete(state, v);
                    return Ok(());
                }
                let previous = u64::from(w.style & 0x10000000 != 0);
                let visible = u64::from(a[1] != 0);
                if visible != 0 {
                    w.style |= 0x10000000
                } else {
                    w.style &= !0x10000000
                }
                if mode == Mode::X86 && previous == visible {
                    call.complete(state, previous);
                    return Ok(());
                }
                (
                    w.procedure,
                    [a[0], 0x18, visible, 0],
                    Operation::Show {
                        window: a[0],
                        previous,
                        stage: 0,
                    },
                )
            } else {
                let operation = if id.timeout() {
                    if a[6] != 0 {
                        memory.check(a[6], mode as usize / 8, WRITE)?;
                    }
                    Operation::TimedMessage {
                        window: a[0],
                        output: a[6],
                        error_on_exit: a[4] as u32 & 0x20 != 0,
                        completed: None,
                    }
                } else {
                    Operation::Message
                };
                (w.procedure, [a[0], a[1], a[2], a[3]], operation)
            }
        };
        let entry = CallFrame::new(mode, block, &args)?;
        let invalidate = if id == W::Show && a[1] != 0 {
            Some(a[0])
        } else {
            None
        };
        let f = Frame {
            saved: Box::new(state.clone()),
            loader_depth: self.callback_depth(),
            call,
            operation,
            block,
            return_sp: entry.sp + mode as u64 / 8 + if mode == Mode::X86 { 16 } else { 0 },
        };
        entry.enter(target, state, memory)?;
        if let Some(window) = invalidate {
            let _ = self.user.invalidate(window, None, true);
        }
        self.threads
            .current_mut()
            .callbacks
            .push(Callback::Window(Box::new(f)));
        memory.clear_fault();
        Ok(())
    }
    pub(super) fn resume_window(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        let Some(Callback::Window(f)) = self.threads.current_mut().callbacks.last_mut() else {
            return Err(unwind::invalid("window return has no continuation"));
        };
        if let Operation::Integer(integer) = &mut f.operation {
            integer.completed.get_or_insert(state.regs[0] as u32);
        }
        if let Operation::TimedMessage { completed, .. } = &mut f.operation {
            completed.get_or_insert(state.regs[0]);
        }
        if matches!(f.operation, Operation::Enumeration(_)) {
            return self.resume_window_enumeration(state, memory);
        }
        let mut f = f.clone();
        let mode = memory.mode();
        let value = state.regs[0];
        let mut next = None;
        let mut client = None;
        let mut ready = None;
        let mut remove = None;
        let result = match &mut f.operation {
            Operation::Message => value,
            Operation::TimedMessage {
                window,
                output,
                error_on_exit,
                completed,
            } => {
                if *error_on_exit && !self.user.windows.contains_key(window) {
                    memory.write_int(
                        crate::win32::context::error_address(&f.saved, mode),
                        4,
                        1400,
                    )?;
                    0
                } else {
                    if *output != 0 {
                        memory.write_int(*output, mode as usize / 8, completed.unwrap())?;
                    }
                    1
                }
            }
            Operation::Timer => 0,
            Operation::Paint(p) => self.finish_paint(p, value != 0, state, memory)?,
            Operation::Position(p) => self.position_resume(p, state, memory, &mut next)?,
            Operation::Integer(integer) => dialog::finish_integer(integer, memory)?,
            Operation::Enumeration(_) => unreachable!("enumeration has a two-argument callback"),
            Operation::Notifications {
                messages,
                next: index,
                result,
            } => {
                while *index < messages.len() {
                    let args = messages[*index];
                    *index += 1;
                    if self.user.windows.contains_key(&args[0]) {
                        next = Some(args);
                        break;
                    }
                }
                *result
            }
            Operation::Create { window, stage } => {
                if self.user.windows.contains_key(window) {
                    match *stage {
                        0 => {
                            *stage = 1;
                            next = Some([*window, 0x81, 0, f.block]);
                        }
                        1 if value as u32 == 0 => {
                            *stage = 5;
                            next = Some([*window, 0x82, 0, 0]);
                        }
                        1 => {
                            *stage = if mode == Mode::X86 { 3 } else { 2 };
                            next = Some([
                                *window,
                                if mode == Mode::X86 { 1 } else { 0x83 },
                                0,
                                if mode == Mode::X86 {
                                    f.block
                                } else {
                                    f.block + 144
                                },
                            ]);
                        }
                        2 => {
                            let r = user::rect(memory, f.block + 144)?;
                            if r[2] < r[0]
                                || r[3] < r[1]
                                || r[2].checked_sub(r[0]).is_none()
                                || r[3].checked_sub(r[1]).is_none()
                            {
                                return Err(unwind::invalid(
                                    "WNDPROC returned invalid client rectangle",
                                ));
                            }
                            client = Some((*window, r));
                            *stage = 3;
                            next = Some([*window, 1, 0, f.block]);
                        }
                        3 if value as u32 == u32::MAX => {
                            *stage = 4;
                            next = Some([*window, 2, 0, 0]);
                        }
                        3 => ready = Some(*window),
                        4 => {
                            *stage = 5;
                            next = Some([*window, 0x82, 0, 0]);
                        }
                        5 => {}
                        _ => return Err(unwind::invalid("invalid window creation continuation")),
                    }
                    if next.is_none() && *stage != 3 {
                        remove = Some(*window);
                    }
                    if *stage == 3 {
                        *window
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            Operation::Destroy {
                window,
                stage,
                result,
            } => {
                if *stage == 0 && self.user.windows.contains_key(window) {
                    *stage = 1;
                    next = Some([*window, 0x82, 0, 0]);
                } else {
                    remove = Some(*window);
                }
                *result
            }
            Operation::Show {
                window,
                previous,
                stage,
            } => {
                if mode == Mode::X64 && *stage == 0 {
                    if let Some(w) = self.user.windows.get(window) {
                        *stage = 1;
                        let size = ((w.client[3] - w.client[1]) as u64 & 65535) << 16
                            | ((w.client[2] - w.client[0]) as u64 & 65535);
                        next = Some([*window, 5, 0, size]);
                    }
                }
                *previous
            }
        };
        if let Some(args) = next {
            let Some(w) = self.user.windows.get(&args[0]) else {
                return Err(unwind::invalid(
                    "window disappeared before its next callback",
                ));
            };
            let call = CallFrame::new(mode, f.block, &args)?;
            call.enter(w.procedure, state, memory)?;
        }
        if let Some((window, rect)) = client {
            self.user.windows.get_mut(&window).unwrap().client = rect;
        }
        if let Some(window) = ready {
            self.user.windows.get_mut(&window).unwrap().ready = true;
            if self.user.windows[&window].style & 0x10000000 != 0 {
                self.user
                    .invalidate(window, None, true)
                    .map_err(|_| unwind::invalid("window disappeared before invalidation"))?;
            }
        }
        if let Some(window) = remove {
            self.user.remove(window);
        }
        if next.is_some() {
            *self.threads.current_mut().callbacks.last_mut().unwrap() = Callback::Window(f);
            return Ok(());
        }
        let (work, retired) = (state.work, state.retired);
        *state = *f.saved;
        state.work = work;
        state.retired = retired;
        f.call.complete(state, result);
        self.threads.current_mut().callbacks.pop();
        memory.clear_fault();
        Ok(())
    }
}
