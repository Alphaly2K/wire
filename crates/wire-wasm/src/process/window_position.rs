/*
 * Default position notifications retain the shared Hangover/Wine derivation.
 * Copyright 1993, 1996 Alexandre Julliard; 1995 Alex Korobka;
 * Copyright 2022 Jacek Caban for CodeWeavers. LGPL-2.1-or-later;
 * LGPL 2.1 selected for wire's combined work. No warranty.
 */
//! Guest WINDOWPOS and NCCALCSIZE records live below the original API stack.
use super::*;

#[derive(Clone)]
pub(super) struct Position {
    window: u64,
    buffer: u64,
    stage: u8,
    old_rect: [i32; 4],
    old_client: [i32; 4],
    new_rect: [i32; 4],
}
fn dimensions(r: [i32; 4]) -> [i32; 2] {
    [r[2] - r[0], r[3] - r[1]]
}
fn packed(x: i32, y: i32) -> u64 {
    u64::from(x as u16) | (u64::from(y as u16) << 16)
}
fn offset(mode: Mode) -> usize {
    if mode == Mode::X86 {
        8
    } else {
        16
    }
}
fn allowed(mode: Mode, after: u64, flags: u32) -> bool {
    flags & !0x7ff == 0
        && (flags & 4 != 0
            || after == mode.address(u64::MAX)
            || after == mode.address(u64::MAX - 1))
}
impl Loaded {
    pub(super) fn position_call(
        &mut self,
        id: W,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), ServiceError> {
        let a = call.arguments;
        let mode = memory.mode();
        let Some(w) = self.user.windows.get(&a[0]) else {
            let v = user::error(memory, state, 1400)?;
            call.complete(state, v);
            return Ok(());
        };
        if w.tid != u64::from(self.threads.active) {
            let v = user::error(memory, state, 50)?;
            call.complete(state, v);
            return Ok(());
        }
        let flags = if id == W::Move {
            0x14 | if a[5] == 0 { 8 } else { 0 }
        } else {
            a[6] as u32
        };
        let after = if id == W::Move { 0 } else { a[1] };
        if !allowed(mode, after, flags) {
            let v = user::error(memory, state, 50)?;
            call.complete(state, v);
            return Ok(());
        }
        if self.threads.current().callbacks.len() >= 32 {
            return Err(ServiceError::Unavailable(
                "USER callback nesting capacity exceeded",
            ));
        }
        user::procedure(memory, services, w.procedure)?;
        let block = (state.regs[4] & !15)
            .checked_sub(128)
            .ok_or(ServiceError::Unavailable("WINDOWPOS stack underflow"))?;
        memory.check(block, 128, WRITE)?;
        let entry = CallFrame::new(mode, block, &[0; 4])?;
        memory.check(entry.sp, entry.bytes.len(), WRITE)?;
        let mut p = Position {
            window: a[0],
            buffer: block,
            stage: 0,
            old_rect: w.rect,
            old_client: w.client,
            new_rect: w.rect,
        };
        let mut bytes = [0; 128];
        let ptr = mode as usize / 8;
        let at = offset(mode);
        bytes[..ptr].copy_from_slice(&a[0].to_le_bytes()[..ptr]);
        bytes[ptr..ptr * 2].copy_from_slice(&after.to_le_bytes()[..ptr]);
        let argument = if id == W::Move { 1 } else { 2 };
        for n in 0..4 {
            bytes[at + n * 4..at + n * 4 + 4]
                .copy_from_slice(&(a[argument + n] as u32).to_le_bytes());
        }
        bytes[at + 16..at + 20].copy_from_slice(&flags.to_le_bytes());
        memory.write(block, &bytes)?;
        let mut next = if flags & 0x400 == 0 {
            Some([a[0], 0x46, 0, block])
        } else {
            None
        };
        if next.is_none() {
            let answer = self
                .position_resume(&mut p, state, memory, &mut next)
                .map_err(|e| match e {
                    StartupError::Fault(f) => ServiceError::Fault(f),
                    _ => ServiceError::Unavailable("invalid initial WINDOWPOS"),
                })?;
            if next.is_none() {
                call.complete(state, answer);
                return Ok(());
            }
        }
        let args = next.unwrap();
        let entry = CallFrame::new(mode, block, &args)?;
        let frame = Frame {
            saved: Box::new(state.clone()),
            loader_depth: self.callback_depth(),
            call,
            operation: Operation::Position(p),
            block,
            return_sp: entry.sp + if mode == Mode::X86 { 20 } else { 8 },
        };
        entry.enter(self.user.windows[&a[0]].procedure, state, memory)?;
        self.threads
            .current_mut()
            .callbacks
            .push(Callback::Window(Box::new(frame)));
        Ok(())
    }
    pub(super) fn position_resume(
        &mut self,
        p: &mut Position,
        state: &State,
        memory: &mut Memory,
        next: &mut Option<[u64; 4]>,
    ) -> Result<u64, StartupError> {
        let Some(w) = self.user.windows.get(&p.window) else {
            return Ok(0);
        };
        if p.stage == 3 {
            return Ok(1);
        }
        let mode = memory.mode();
        let ptr = mode as usize / 8;
        let at = offset(mode);
        let rect_at = if mode == Mode::X86 { 32 } else { 48 };
        let mut bytes = [0; 128];
        memory.read(p.buffer, &mut bytes)?;
        let dword = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let pointer = |at: usize| {
            let mut b = [0; 8];
            b[..ptr].copy_from_slice(&bytes[at..at + ptr]);
            u64::from_le_bytes(b)
        };
        let mut flags = dword(at + 16);
        let after = pointer(ptr);
        if pointer(0) != p.window || !allowed(mode, after, flags) {
            return Err(unwind::invalid(
                "WNDPROC returned unsupported WINDOWPOS identity or flags",
            ));
        }
        if p.stage == 0 {
            let old = w.rect;
            let old_size = dimensions(old);
            let x = if flags & 2 != 0 {
                old[0]
            } else {
                dword(at) as i32
            };
            let y = if flags & 2 != 0 {
                old[1]
            } else {
                dword(at + 4) as i32
            };
            let cx = if flags & 1 != 0 {
                old_size[0]
            } else {
                dword(at + 8) as i32
            };
            let cy = if flags & 1 != 0 {
                old_size[1]
            } else {
                dword(at + 12) as i32
            };
            if !(0..=8192).contains(&cx)
                || !(0..=8192).contains(&cy)
                || x.checked_add(cx).is_none()
                || y.checked_add(cy).is_none()
            {
                memory.write_int(crate::win32::context::error_address(state, mode), 4, 87)?;
                return Ok(0);
            }
            p.new_rect = [x, y, x + cx, y + cy];
            for (n, value) in [x, y, cx, cy].into_iter().enumerate() {
                bytes[at + n * 4..at + n * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
            let calculate = [cx, cy] != dimensions(p.old_rect) || flags & 0x20 != 0;
            let client = if calculate {
                p.new_rect
            } else {
                std::array::from_fn(|n| {
                    let axis = n % 2;
                    p.new_rect[axis].wrapping_add(p.old_client[n].wrapping_sub(p.old_rect[axis]))
                })
            };
            for (n, rect) in [client, p.old_rect, p.old_client].into_iter().enumerate() {
                for (i, v) in rect.into_iter().enumerate() {
                    let index = rect_at + n * 16 + i * 4;
                    bytes[index..index + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
            bytes[rect_at + 48..rect_at + 48 + ptr].copy_from_slice(&p.buffer.to_le_bytes()[..ptr]);
            memory.write(p.buffer, &bytes)?;
            if calculate {
                p.stage = 1;
                *next = Some([p.window, 0x83, 1, p.buffer + rect_at as u64]);
                return Ok(1);
            }
        }
        let client = std::array::from_fn(|n| {
            i32::from_le_bytes(
                bytes[rect_at + n * 4..rect_at + n * 4 + 4]
                    .try_into()
                    .unwrap(),
            )
        });
        if !(0..=8192).contains(&(i64::from(client[2]) - i64::from(client[0])))
            || !(0..=8192).contains(&(i64::from(client[3]) - i64::from(client[1])))
        {
            return Err(unwind::invalid(
                "WNDPROC returned an invalid WINDOWPOS client rectangle",
            ));
        }
        if client[..2] == p.old_client[..2] {
            flags |= 0x1000;
        }
        if dimensions(client) == dimensions(p.old_client) {
            flags |= 0x800;
        }
        bytes[at + 16..at + 20].copy_from_slice(&flags.to_le_bytes());
        let entry = CallFrame::new(mode, p.buffer, &[p.window, 0x47, 0, p.buffer])?;
        memory.check(entry.sp, entry.bytes.len(), WRITE)?;
        memory.check(p.buffer, bytes.len(), WRITE)?;
        if let Err(code) = self.user.gdi.resize(
            p.window,
            (client[2] - client[0]) as u32,
            (client[3] - client[1]) as u32,
        ) {
            memory.write_int(
                crate::win32::context::error_address(state, mode),
                4,
                u64::from(code),
            )?;
            return Ok(0);
        }
        memory.write(p.buffer, &bytes)?;
        let w = self.user.windows.get_mut(&p.window).unwrap();
        w.rect = p.new_rect;
        w.client = client;
        if flags & 4 == 0 {
            if after == mode.address(u64::MAX) {
                w.ex_style |= 8;
            } else {
                w.ex_style &= !8;
            }
        }
        if flags & 0x40 != 0 {
            w.style |= 0x10000000;
        }
        if flags & 0x80 != 0 {
            w.style &= !0x10000000;
        }
        if flags & 0x10 == 0 && w.style & 0x10000000 != 0 {
            self.user
                .input
                .entry(self.threads.active)
                .or_default()
                .active = p.window;
        }
        if flags & 8 == 0 {
            let _ = self.user.invalidate(p.window, None, true);
        }
        p.stage = 3;
        *next = Some([p.window, 0x47, 0, p.buffer]);
        Ok(1)
    }
    pub(super) fn default_position(
        &mut self,
        call: Call,
        state: &mut State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<(), ServiceError> {
        let a = call.arguments;
        let Some(w) = self.user.windows.get(&a[0]) else {
            let v = user::error(memory, state, 1400)?;
            call.complete(state, v);
            return Ok(());
        };
        if w.tid != u64::from(self.threads.active) {
            let v = user::error(memory, state, 50)?;
            call.complete(state, v);
            return Ok(());
        }
        let flags = memory.read_int(
            a[3].checked_add((offset(memory.mode()) + 16) as u64)
                .ok_or(ServiceError::Unavailable("WINDOWPOS address overflow"))?,
            4,
        )? as u32;
        let mut messages = Vec::new();
        if flags & 0x1000 == 0 {
            messages.push([a[0], 3, 0, packed(w.client[0], w.client[1])]);
        }
        if flags & 0x800 == 0 || flags & 0x8000 != 0 {
            let (kind, size) = if w.style & 0x20000000 != 0 {
                (1, 0)
            } else {
                let size = dimensions(w.client);
                (
                    if w.style & 0x1000000 != 0 { 2 } else { 0 },
                    packed(size[0], size[1]),
                )
            };
            messages.push([a[0], 5, kind, size]);
        }
        self.notifications(call, state, memory, services, messages, 0)
    }
}
