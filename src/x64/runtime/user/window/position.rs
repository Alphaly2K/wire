/*
 * Default WM_WINDOWPOSCHANGED rules adapted from Hangover Wine win32u/defwnd.c.
 * Fixed reference: third_party/hangover/windowpos.c (3ea3a14941b6a122b892c84141ce9662c008081e).
 * Copyright 1993, 1996 Alexandre Julliard
 * Copyright 1995 Alex Korobka
 * Copyright 2022 Jacek Caban for CodeWeavers
 * GNU Lesser General Public License, version 2.1 or later. No warranty.
 */
//! Win64 WINDOWPOS/NCCALCSIZE layouts and resumable positioning callbacks.
use super::{
    rectangle, write_rect, Memory, Operation, Runtime64, WindowCall, WindowResult, WindowStep,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Position {
    window: u64,
    buffer: u64,
    stage: u8,
    old_rect: [i32; 4],
    old_client: [i32; 4],
    new_rect: [i32; 4],
}
fn dimensions(rect: [i32; 4]) -> (i32, i32) {
    (rect[2] - rect[0], rect[3] - rect[1])
}
fn packed(x: i32, y: i32) -> u64 {
    (x as u32 as u64 & 0xffff) | ((y as u32 as u64 & 0xffff) << 16)
}
impl Runtime64 {
    pub(super) fn position_start(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<WindowResult, String> {
        let Some(w) = self.user.windows.get(&args[0]) else {
            self.set_error(mem, 1400)?;
            return Ok(WindowResult::Value(0));
        };
        if w.tid != self.threads.active {
            return Err("cross-thread Win64 window positioning is not implemented".into());
        }
        let flags = if name == "MoveWindow" {
            0x14
        } else {
            args[6] as u32
        };
        // Native z-order/asynchronous transitions are not connected yet.
        if flags & !0x7ff != 0 || flags & 4 == 0 {
            self.set_error(mem, 50)?;
            return Ok(WindowResult::Value(0));
        }
        let mut position = Position {
            window: args[0],
            buffer: 0,
            stage: 0,
            old_rect: w.rect,
            old_client: w.client,
            new_rect: w.rect,
        };
        let procedure = w.procedure;
        let offset = if name == "MoveWindow" { 1 } else { 2 };
        let mut bytes = [0; 128];
        bytes[..8].copy_from_slice(&args[0].to_le_bytes());
        if name != "MoveWindow" {
            bytes[8..16].copy_from_slice(&args[1].to_le_bytes());
        }
        for n in 0..4 {
            bytes[16 + 4 * n..20 + 4 * n].copy_from_slice(&(args[offset + n] as u32).to_le_bytes());
        }
        bytes[32..36].copy_from_slice(&flags.to_le_bytes());
        position.buffer = self.heap_alloc(mem, bytes.len() as u64)?;
        if position.buffer == 0 {
            return Ok(WindowResult::Value(0));
        }
        mem.write(position.buffer, &bytes)?;
        let parameters = if flags & 0x400 == 0 {
            [args[0], 0x46, 0, position.buffer]
        } else {
            let step = self.position_resume(mem, &mut position);
            if step.is_err() {
                self.release_callback_buffers(mem, &[position.buffer])?;
            }
            match step? {
                WindowStep::Next(_, parameters) => parameters,
                WindowStep::Returned(value) => {
                    self.release_callback_buffers(mem, &[position.buffer])?;
                    return Ok(WindowResult::Value(value));
                }
            }
        };
        Ok(WindowResult::Callback {
            target: procedure,
            parameters,
            release: vec![position.buffer],
            call: WindowCall {
                operation: Operation::Position(position),
                name: name.into(),
                args: args.to_vec(),
            },
        })
    }
    pub(super) fn position_resume(
        &mut self,
        mem: &mut Memory,
        position: &mut Position,
    ) -> Result<WindowStep, String> {
        let Some(w) = self.user.windows.get(&position.window) else {
            return Ok(WindowStep::Returned(0));
        };
        let procedure = w.procedure;
        if position.stage == 3 {
            return Ok(WindowStep::Returned(1));
        }
        let buffer = position.buffer;
        let mut flags = mem.read_int(buffer + 32, 4)? as u32;
        if mem.read_int(buffer, 8)? != position.window || flags & !0x7ff != 0 || flags & 4 == 0 {
            return Err("WNDPROC returned unsupported WINDOWPOS flags or changed its HWND".into());
        }
        if position.stage == 0 {
            let old = w.rect;
            let (old_width, old_height) = dimensions(old);
            let x = if flags & 2 != 0 {
                old[0]
            } else {
                mem.read_int(buffer + 16, 4)? as i32
            };
            let y = if flags & 2 != 0 {
                old[1]
            } else {
                mem.read_int(buffer + 20, 4)? as i32
            };
            let cx = if flags & 1 != 0 {
                old_width
            } else {
                mem.read_int(buffer + 24, 4)? as i32
            };
            let cy = if flags & 1 != 0 {
                old_height
            } else {
                mem.read_int(buffer + 28, 4)? as i32
            };
            if !(0..=8192).contains(&cx) || !(0..=8192).contains(&cy) {
                self.set_error(mem, 87)?;
                return Ok(WindowStep::Returned(0));
            }
            position.new_rect = [
                x,
                y,
                x.checked_add(cx).ok_or("WINDOWPOS x overflow")?,
                y.checked_add(cy).ok_or("WINDOWPOS y overflow")?,
            ];
            // WM_WINDOWPOSCHANGED receives normalized values for ignored fields.
            for (n, value) in [x, y, cx, cy].into_iter().enumerate() {
                mem.write_int(buffer + 16 + n as u64 * 4, 4, value as u32 as u64)?;
            }
            position.stage = 1;
            if (cx, cy) != dimensions(position.old_rect) || flags & 0x20 != 0 {
                write_rect(mem, buffer + 48, position.new_rect)?;
                write_rect(mem, buffer + 64, position.old_rect)?;
                write_rect(mem, buffer + 80, position.old_client)?;
                mem.write_int(buffer + 96, 8, buffer)?;
                return Ok(WindowStep::Next(
                    procedure,
                    [position.window, 0x83, 1, buffer + 48],
                ));
            }
            let rect = std::array::from_fn(|n| {
                let axis = n % 2;
                position.new_rect[axis]
                    .wrapping_add(position.old_client[n].wrapping_sub(position.old_rect[axis]))
            });
            write_rect(mem, buffer + 48, rect)?;
        }
        let client = rectangle(mem, buffer + 48)?;
        let width = client[2] as i64 - client[0] as i64;
        let height = client[3] as i64 - client[1] as i64;
        if !(0..=8192).contains(&width) || !(0..=8192).contains(&height) {
            return Err("WNDPROC returned an invalid WINDOWPOS client rectangle".into());
        }
        if client[..2] == position.old_client[..2] {
            flags |= 0x1000;
        }
        if dimensions(client) == dimensions(position.old_client) {
            flags |= 0x800;
        }
        mem.write_int(buffer + 32, 4, flags as u64)?;
        let w = self.user.windows.get_mut(&position.window).unwrap();
        w.rect = position.new_rect;
        w.client = client;
        if flags & 0x40 != 0 {
            w.style |= 0x10000000;
        }
        if flags & 0x80 != 0 {
            w.style &= !0x10000000;
        }
        if flags & 0x10 == 0 && w.style & 0x10000000 != 0 {
            self.user.active = position.window;
        }
        position.stage = 3;
        Ok(WindowStep::Next(
            procedure,
            [position.window, 0x47, 0, buffer],
        ))
    }
    // Same message rules as the LGPL Hangover reference used by the PE32
    // default_windowpos_changed implementation; Windows layouts here are Win64.
    pub(super) fn default_position_start(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<WindowResult, String> {
        let flags = mem.read_int(args[3].checked_add(32).ok_or("WINDOWPOS overflow")?, 4)? as u32;
        let Some(w) = self.user.windows.get(&args[0]) else {
            self.set_error(mem, 1400)?;
            return Ok(WindowResult::Value(0));
        };
        if w.tid != self.threads.active {
            return Err("cross-thread default window callback is not implemented".into());
        }
        let mut messages = Vec::new();
        if flags & 0x1000 == 0 {
            messages.push([args[0], 3, 0, packed(w.client[0], w.client[1])]);
        }
        if flags & 0x800 == 0 || flags & 0x8000 != 0 {
            let (parameter, size) = if w.style & 0x20000000 != 0 {
                (1, 0)
            } else {
                let (cx, cy) = dimensions(w.client);
                (
                    if w.style & 0x01000000 != 0 { 2 } else { 0 },
                    packed(cx, cy),
                )
            };
            messages.push([args[0], 5, parameter, size]);
        }
        if messages.is_empty() {
            return Ok(WindowResult::Value(0));
        }
        Ok(WindowResult::Callback {
            target: w.procedure,
            parameters: messages[0],
            release: Vec::new(),
            call: WindowCall {
                operation: Operation::DefaultPosition {
                    window: args[0],
                    messages,
                    next: 1,
                },
                name: name.into(),
                args: args.to_vec(),
            },
        })
    }
}
