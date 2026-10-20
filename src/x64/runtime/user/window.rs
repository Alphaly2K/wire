//! Win64 window creation and WNDPROC transitions resumed by the guest executor.
use super::{rectangle, write_rect, Memory, Runtime64, User, Window};
use crate::x64::runtime::threads::Threads;
use crate::x64::{
    cpu::Cpu64,
    memory::{EXECUTE, READ, WRITE},
    runtime::WindowStep,
};
mod position;
mod queries;
mod state;
use crate::win32::catalog::WindowControlApi as W;
fn valid_procedure(
    mem: &Memory,
    gateways: &crate::x64::runtime::gateways::Gateways,
    procedure: u64,
) -> Result<(), String> {
    if gateways.contains_key(&procedure) {
        Ok(())
    } else {
        mem.check(procedure, 1, READ | EXECUTE)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Operation {
    Create {
        window: u64,
        buffer: u64,
        stage: u8,
    },
    Message,
    Destroy {
        window: u64,
        stage: u8,
    },
    Show {
        window: u64,
        previous: u64,
        stage: u8,
    },
    Position(position::Position),
    DefaultPosition {
        window: u64,
        messages: Vec<[u64; 4]>,
        next: usize,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowCall {
    operation: Operation,
    name: String,
    args: Vec<u64>,
}
pub(in crate::x64::runtime) enum WindowResult {
    Value(u64),
    Callback {
        target: u64,
        parameters: [u64; 4],
        call: WindowCall,
        release: Vec<u64>,
    },
}
impl Runtime64 {
    pub fn window_info(&self) -> Vec<super::super::WindowInfo> {
        self.user.window_info()
    }
    pub fn queue_window_event(&mut self, event: super::super::WindowEvent) -> Result<(), String> {
        self.user.queue_window_event(event, self.start)
    }
    pub fn take_window_events(&mut self) -> Vec<super::super::WindowEvent> {
        self.user.take_window_events()
    }
    fn valid_procedure(&self, mem: &Memory, procedure: u64) -> Result<(), String> {
        valid_procedure(mem, &self.gateways, procedure)
    }
    fn prepare_window_callback(&self, mem: &Memory, returned: &Cpu64) -> Result<(), String> {
        if !self.threads.enabled {
            return Err("Win64 WNDPROC requires a process executor".into());
        }
        let frame = (returned.regs[4]
            .checked_sub(48)
            .ok_or("WNDPROC stack underflow")?
            & !15)
            + 8;
        mem.check(frame, 40, WRITE)?;
        Ok(())
    }
    pub(in crate::x64::runtime) fn dispatch_window(
        &mut self,
        mem: &mut Memory,
        id: crate::win32::catalog::MessageApi,
        name: &str,
        args: &[u64],
        returned: &Cpu64,
    ) -> Result<WindowResult, String> {
        self.prepare_window_callback(mem, returned)?;
        let wide = id == crate::win32::catalog::MessageApi::DispatchW;
        let words = super::message::read(mem, args[0])?;
        if words[0] == 0 {
            return Ok(WindowResult::Value(0));
        }
        if words[1] == 0x113 && words[3] != 0 {
            return Err("Win64 timer-procedure dispatch is not implemented".into());
        }
        let mut result = self.window_start(
            mem,
            if wide { W::SendW } else { W::SendA },
            if wide { "SendMessageW" } else { "SendMessageA" },
            &words[..4],
            returned,
        )?;
        if let WindowResult::Callback { call, .. } = &mut result {
            call.name = name.into();
            call.args = args.to_vec();
        }
        Ok(result)
    }
    pub(in crate::x64::runtime) fn window_start(
        &mut self,
        mem: &mut Memory,
        id: W,
        name: &str,
        args: &[u64],
        returned: &Cpu64,
    ) -> Result<WindowResult, String> {
        let wide = id.wide();
        self.prepare_window_callback(mem, returned)?;
        if id.default_procedure() {
            if args[1] == 0x47 {
                return self.default_position_start(mem, name, args);
            }
            return self
                .user
                .window_value_call(mem, id, args, &self.threads)
                .map(WindowResult::Value);
        }
        if matches!(id, W::Position | W::Move) {
            return self.position_start(mem, id, name, args);
        }
        let (target, parameters, operation, release) = if matches!(id, W::CreateA | W::CreateW) {
            let key = super::super::resources::resource_key(mem, args[1], wide)?;
            let key = match &key {
                super::super::resources::Key::Id(id) => crate::win32::user::classes64::Key::Id(*id),
                super::super::resources::Key::Name(name) => {
                    crate::win32::user::classes64::Key::Name(name)
                }
            };
            let class = self
                .user
                .classes
                .find(args[10], key, true)
                .map(|(atom, class)| (atom, class.clone()));
            let Some((atom, class)) = class else {
                self.set_error(mem, 1407)?;
                return Ok(WindowResult::Value(0));
            };
            if args[8] != 0 || args[9] != 0 || args[3] & 0x40000000 != 0 {
                self.set_error(mem, 50)?;
                return Ok(WindowResult::Value(0));
            }
            self.valid_procedure(mem, class.procedure)?;
            let title = if args[2] == 0 {
                String::new()
            } else {
                Self::path_name(mem, args[2], wide)?
            };
            let mut dimensions: [i32; 4] = std::array::from_fn(|i| args[4 + i] as u32 as i32);
            for (i, default) in [100, 100, 800, 600].into_iter().enumerate() {
                if dimensions[i] == i32::MIN {
                    dimensions[i] = default;
                }
            }
            if dimensions[2] <= 0
                || dimensions[3] <= 0
                || dimensions[2] > 8192
                || dimensions[3] > 8192
            {
                self.set_error(mem, 87)?;
                return Ok(WindowResult::Value(0));
            }
            if self.user.windows.len() >= 1024
                || self.user.next_window >= super::super::TEB + 0x640000
            {
                self.set_error(mem, 8)?;
                return Ok(WindowResult::Value(0));
            }
            let rect = [
                dimensions[0],
                dimensions[1],
                dimensions[0].wrapping_add(dimensions[2]),
                dimensions[1].wrapping_add(dimensions[3]),
            ];
            let buffer = self.heap_alloc(mem, 160)?;
            if buffer == 0 {
                return Ok(WindowResult::Value(0));
            }
            let mut bytes = [0; 160];
            for (offset, value) in [
                (0, args[11]),
                (8, args[10]),
                (16, args[9]),
                (24, args[8]),
                (56, args[2]),
                (64, args[1]),
            ] {
                bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
            for (offset, value) in [
                (32, dimensions[3] as u32),
                (36, dimensions[2] as u32),
                (40, dimensions[1] as u32),
                (44, dimensions[0] as u32),
                (48, args[3] as u32),
                (72, args[0] as u32),
                (96 + 24, 1024),
                (96 + 28, 768),
                (96 + 32, 1),
                (96 + 36, 1),
            ] {
                bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            for (i, value) in rect.into_iter().enumerate() {
                bytes[144 + 4 * i..148 + 4 * i].copy_from_slice(&value.to_le_bytes());
            }
            mem.write(buffer, &bytes)?;
            let handle = self.user.next_window;
            self.user.next_window += 16;
            self.user.windows.insert(
                handle,
                Window {
                    class: atom,
                    procedure: class.procedure,
                    instance: args[10],
                    parent: 0,
                    menu: 0,
                    style: args[3] as u32,
                    ex_style: args[0] as u32,
                    rect,
                    client: rect,
                    data: 0,
                    extra: vec![0; class.window_extra],
                    tid: self.threads.active,
                    wide,
                    title,
                    ready: false,
                },
            );
            (
                class.procedure,
                [handle, 0x24, 0, buffer + 96],
                Operation::Create {
                    window: handle,
                    buffer,
                    stage: 0,
                },
                vec![buffer],
            )
        } else if matches!(id, W::CallA | W::CallW) {
            self.valid_procedure(mem, args[0])?;
            (
                args[0],
                [args[1], args[2], args[3], args[4]],
                Operation::Message,
                Vec::new(),
            )
        } else {
            let Some(window) = self.user.windows.get_mut(&args[0]) else {
                self.set_error(mem, 1400)?;
                return Ok(WindowResult::Value(0));
            };
            let procedure = window.procedure;
            if window.tid != self.threads.active {
                return Err("cross-thread Win64 window call is not implemented".into());
            }
            if id == W::Destroy {
                (
                    procedure,
                    [args[0], 2, 0, 0],
                    Operation::Destroy {
                        window: args[0],
                        stage: 0,
                    },
                    Vec::new(),
                )
            } else if id == W::Show {
                if !matches!(args[1], 0 | 1 | 4 | 5 | 8) {
                    self.set_error(mem, 50)?;
                    return Ok(WindowResult::Value(0));
                }
                let previous = (window.style & 0x10000000 != 0) as u64;
                let visible = args[1] != 0;
                if visible {
                    window.style |= 0x10000000;
                } else {
                    window.style &= !0x10000000;
                }
                (
                    procedure,
                    [args[0], 0x18, visible as u64, 0],
                    Operation::Show {
                        window: args[0],
                        previous,
                        stage: 0,
                    },
                    Vec::new(),
                )
            } else {
                (
                    procedure,
                    [args[0], args[1], args[2], args[3]],
                    Operation::Message,
                    Vec::new(),
                )
            }
        };
        Ok(WindowResult::Callback {
            target,
            parameters,
            call: WindowCall {
                operation,
                name: name.into(),
                args: args.to_vec(),
            },
            release,
        })
    }
    pub(crate) fn window_resume(
        &mut self,
        mem: &mut Memory,
        call: &mut WindowCall,
        value: u64,
    ) -> Result<WindowStep, String> {
        let result = match &mut call.operation {
            Operation::Position(position) => match self.position_resume(mem, position)? {
                WindowStep::Next(target, args) => return Ok(WindowStep::Next(target, args)),
                WindowStep::Returned(value) => value,
            },
            Operation::DefaultPosition {
                window,
                messages,
                next,
            } => {
                if let Some(w) = self.user.windows.get(window) {
                    if let Some(&parameters) = messages.get(*next) {
                        *next += 1;
                        return Ok(WindowStep::Next(w.procedure, parameters));
                    }
                }
                0
            }
            Operation::Message => value,
            Operation::Create {
                window,
                buffer,
                stage,
            } => {
                let handle = *window;
                let procedure = self
                    .user
                    .windows
                    .get(&handle)
                    .ok_or("window removed during creation callback")?
                    .procedure;
                let next = match *stage {
                    0 => {
                        *stage = 1;
                        Some([handle, 0x81, 0, *buffer])
                    }
                    1 if value as u32 != 0 => {
                        *stage = 2;
                        Some([handle, 0x83, 0, *buffer + 144])
                    }
                    1 => {
                        *stage = 5;
                        Some([handle, 0x82, 0, 0])
                    }
                    2 => {
                        let client = rectangle(mem, *buffer + 144)?;
                        if client[2] < client[0] || client[3] < client[1] {
                            return Err("WNDPROC returned invalid client rectangle".into());
                        }
                        self.user.windows.get_mut(&handle).unwrap().client = client;
                        *stage = 3;
                        Some([handle, 1, 0, *buffer])
                    }
                    3 if value as u32 == u32::MAX => {
                        *stage = 4;
                        Some([handle, 2, 0, 0])
                    }
                    3 => {
                        self.user.windows.get_mut(&handle).unwrap().ready = true;
                        self.user.active = handle;
                        None
                    }
                    4 => {
                        *stage = 5;
                        Some([handle, 0x82, 0, 0])
                    }
                    5 => {
                        self.remove_window(handle);
                        None
                    }
                    _ => return Err("invalid window creation phase".into()),
                };
                if let Some(parameters) = next {
                    return Ok(WindowStep::Next(procedure, parameters));
                }
                if *stage == 3 {
                    handle
                } else {
                    0
                }
            }
            Operation::Destroy { window, stage } => {
                if *stage == 0 {
                    let procedure = self
                        .user
                        .windows
                        .get(window)
                        .ok_or("window removed during destroy callback")?
                        .procedure;
                    *stage = 1;
                    return Ok(WindowStep::Next(procedure, [*window, 0x82, 0, 0]));
                }
                self.remove_window(*window);
                1
            }
            Operation::Show {
                window,
                previous,
                stage,
            } => {
                if *stage == 0 {
                    let w = self
                        .user
                        .windows
                        .get(window)
                        .ok_or("window removed during show callback")?;
                    let size = ((w.client[3] - w.client[1]) as u32 as u64 & 65535) << 16
                        | ((w.client[2] - w.client[0]) as u32 as u64 & 65535);
                    *stage = 1;
                    return Ok(WindowStep::Next(w.procedure, [*window, 5, 0, size]));
                }
                *previous
            }
        };
        self.complete_call("user32.dll", &call.name, &call.args, result);
        Ok(WindowStep::Returned(result))
    }
    fn remove_window(&mut self, handle: u64) {
        self.remove_device_notifications(handle);
        self.user.windows.remove(&handle);
        self.user.events.retain(|m| m.event.window != handle);
        self.user.keyboard.remove_window64(handle);
        if self.user.focus == handle {
            self.user.focus = 0;
        }
        if self.user.active == handle {
            self.user.active = 0;
        }
        if self.user.foreground == handle {
            self.user.foreground = 0;
        }
    }
}
impl User {
    pub(in crate::x64::runtime) fn window_value_call(
        &mut self,
        mem: &mut Memory,
        id: W,
        args: &[u64],
        threads: &Threads,
    ) -> Result<u64, String> {
        let Some(w) = self.windows.get_mut(&args[0]) else {
            mem.write_int(threads.teb() + 0x68, 4, 1400)?;
            return Ok(0);
        };
        match id {
            W::DefaultA | W::DefaultW => match args[1] {
                0x81 => Ok(1),
                0x83 => {
                    if args[2] > 1 {
                        return Err("invalid NCCALCSIZE wparam".into());
                    }
                    let mut rect = rectangle(mem, args[3])?;
                    let frame =
                        crate::win32::user::geometry::frame(w.style, w.ex_style, w.menu != 0, true);
                    for i in 0..4 {
                        rect[i] = if i < 2 {
                            rect[i].wrapping_add(frame[i])
                        } else {
                            rect[i].wrapping_sub(frame[i])
                        };
                    }
                    write_rect(mem, args[3], rect)?;
                    Ok(0)
                }
                _ => Ok(0),
            },
            W::Update => Ok(1), // No Win64 GDI update region has been invalidated yet.
            _ => unreachable!(),
        }
    }
}
