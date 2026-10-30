/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; LGPL 2.1 selected for wire's combined work.
 * Retains the existing Wine USER control and enumeration derivation.
 */
//! Control wrappers and snapshot callbacks use process-owned USER state and the same JIT.
use super::*;
use crate::win32::catalog::DialogApi as D;

#[derive(Clone)]
pub(super) struct Enumeration {
    remaining: VecDeque<u64>,
    callback: u64,
    param: u64,
    children: bool,
    completed: Option<u32>,
}
#[derive(Clone)]
pub(super) struct Integer {
    buffer: u64,
    translated: u64,
    signed: bool,
    wide: bool,
    pub completed: Option<u32>,
}
pub(super) fn finish_integer(integer: &Integer, memory: &mut Memory) -> Result<u64, StartupError> {
    let count = integer.completed.expect("completed WM_GETTEXT").min(29);
    let mut units = Vec::new();
    for n in 0..count {
        let unit = memory.read_int(
            integer.buffer + u64::from(n) * if integer.wide { 2 } else { 1 },
            if integer.wide { 2 } else { 1 },
        )? as u16;
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    let text = String::from_utf16_lossy(&units);
    let value = user::dialog_values::integer(&text, integer.signed);
    if integer.translated != 0 {
        memory.write_int(integer.translated, 4, value.is_some().into())?;
    }
    Ok(value.unwrap_or(0).into())
}

impl Loaded {
    pub(crate) fn dialog_call(
        &mut self,
        id: D,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), ServiceError> {
        let a = call.arguments;
        if id.unavailable() || id.message_box() {
            user::error(memory, state, 120)?;
            let value = if id.modal() {
                memory.mode().address(u64::MAX)
            } else {
                0
            };
            call.complete(state, value);
            return Ok(());
        }
        let links = || self.user.windows.iter().map(|(&h, w)| (h, w.link()));
        if matches!(id, D::Windows | D::ThreadWindows | D::Children) {
            let children = id == D::Children;
            let (callback, param) = if id == D::Windows {
                (a[0], a[1])
            } else {
                (a[1], a[2])
            };
            if !children && callback == 0 {
                let value = user::error(memory, state, 87)?;
                call.complete(state, value);
                return Ok(());
            }
            let windows = if children {
                user::hierarchy::children(links(), a[0])
            } else {
                user::hierarchy::top(links(), if id == D::ThreadWindows { a[0] } else { 0 })
            };
            if windows.is_empty() {
                call.complete(state, 1);
                return Ok(());
            }
            if self.threads.current().callbacks.len() >= 32 {
                return Err(ServiceError::Unavailable(
                    "USER callback nesting capacity exceeded",
                ));
            }
            user::procedure(memory, services, callback)?;
            let mut remaining: VecDeque<_> = windows.into();
            let first = remaining.pop_front().unwrap();
            let block = state.regs[4] & !15;
            let entry = CallFrame::new(memory.mode(), block, &[first, param])?;
            let f = Frame {
                saved: Box::new(state.clone()),
                loader_depth: self.callback_depth(),
                call,
                operation: Operation::Enumeration(Enumeration {
                    remaining,
                    callback,
                    param,
                    children,
                    completed: None,
                }),
                block,
                return_sp: entry.sp + if memory.mode() == Mode::X86 { 12 } else { 8 },
            };
            entry.enter(callback, state, memory)?;
            self.threads
                .current_mut()
                .callbacks
                .push(Callback::Window(Box::new(f)));
            return Ok(());
        }
        let hwnd = if id == D::ControlId {
            a[0]
        } else {
            user::hierarchy::child(links(), a[0], a[1]).unwrap_or(0)
        };
        if id == D::Item {
            call.complete(state, hwnd);
            return Ok(());
        }
        if id == D::ControlId {
            call.complete(
                state,
                self.user
                    .windows
                    .get(&hwnd)
                    .map_or(0, |w| u64::from(w.menu as u32)),
            );
            return Ok(());
        }
        let Some(w) = self.user.windows.get(&hwnd) else {
            let value = user::error(memory, state, 1400)?;
            call.complete(state, value);
            return Ok(());
        };
        if w.tid != u64::from(self.threads.active) {
            let value = user::error(memory, state, 120)?;
            call.complete(state, value);
            return Ok(());
        }
        let numeric = matches!(id, D::GetInteger | D::SetInteger);
        let wide = if numeric {
            w.wide
        } else {
            matches!(id, D::GetTextW | D::SetTextW | D::SendW)
        };
        let message = if id.send() {
            a[2]
        } else if id.set_text() || id == D::SetInteger {
            0xc
        } else {
            0xd
        };
        if wide != w.wide && matches!(message, 0xc..=0xe) {
            let value = user::error(memory, state, 50)?;
            call.complete(state, value);
            return Ok(());
        }
        if self.threads.current().callbacks.len() >= 32 {
            return Err(ServiceError::Unavailable(
                "USER callback nesting capacity exceeded",
            ));
        }
        let target = w.procedure;
        user::procedure(memory, services, target)?;
        let block = (state.regs[4] & !15)
            .checked_sub(if numeric { 64 } else { 0 })
            .ok_or(ServiceError::Unavailable("dialog stack underflow"))?;
        let (wp, lp) = if id.send() {
            (a[3], a[4])
        } else if numeric {
            (if id == D::GetInteger { 30 } else { 0 }, block)
        } else if id.set_text() {
            (0, a[2])
        } else {
            (a[3], a[2])
        };
        let entry = CallFrame::new(memory.mode(), block, &[hwnd, message, wp, lp])?;
        memory.check(entry.sp, entry.bytes.len(), WRITE)?;
        if numeric {
            memory.check(block, 64, WRITE)?;
        }
        let operation = if id == D::GetInteger {
            if a[2] != 0 {
                memory.write_int(a[2], 4, 0)?;
            }
            Operation::Integer(Integer {
                buffer: block,
                translated: a[2],
                signed: a[3] != 0,
                wide,
                completed: None,
            })
        } else {
            if id == D::SetInteger {
                let text = user::dialog_values::text(a[2] as u32, a[3] != 0);
                let bytes: Vec<_> = if wide {
                    text.encode_utf16()
                        .chain(Some(0))
                        .flat_map(u16::to_le_bytes)
                        .collect()
                } else {
                    text.bytes().chain(Some(0)).collect()
                };
                memory.write(block, &bytes)?;
            }
            Operation::Message
        };
        let f = Frame {
            saved: Box::new(state.clone()),
            loader_depth: self.callback_depth(),
            call,
            operation,
            block,
            return_sp: entry.sp + if memory.mode() == Mode::X86 { 20 } else { 8 },
        };
        entry.enter(target, state, memory)?;
        self.threads
            .current_mut()
            .callbacks
            .push(Callback::Window(Box::new(f)));
        Ok(())
    }
    pub(super) fn resume_window_enumeration(
        &mut self,
        state: &mut State,
        memory: &mut Memory,
    ) -> Result<(), StartupError> {
        let Some(Callback::Window(f)) = self.threads.current_mut().callbacks.last_mut() else {
            return Err(unwind::invalid("enumeration return has no continuation"));
        };
        let Operation::Enumeration(e) = &mut f.operation else {
            return Err(unwind::invalid("enumeration return has another operation"));
        };
        let result = *e.completed.get_or_insert(state.regs[0] as u32);
        let mut f = f.clone();
        let Operation::Enumeration(e) = &mut f.operation else {
            unreachable!()
        };
        let mut restored = *f.saved.clone();
        restored.work = state.work;
        restored.retired = state.retired;
        if result != 0 {
            while let Some(window) = e.remaining.pop_front() {
                if self.user.windows.contains_key(&window) {
                    let entry = CallFrame::new(memory.mode(), f.block, &[window, e.param])?;
                    e.completed = None;
                    entry.enter(e.callback, &mut restored, memory)?;
                    *self.threads.current_mut().callbacks.last_mut().unwrap() = Callback::Window(f);
                    *state = restored;
                    memory.clear_fault();
                    return Ok(());
                }
            }
        }
        let result = if e.children { 1 } else { result };
        f.call.complete(&mut restored, result.into());
        self.threads.current_mut().callbacks.pop();
        *state = restored;
        memory.clear_fault();
        Ok(())
    }
}
