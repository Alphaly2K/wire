//! Win64 window creation and WNDPROC transitions resumed by the guest executor.
use super::{rectangle, write_rect, Memory, Runtime64, Window};
use crate::x64::{
    cpu::Cpu64,
    memory::{EXECUTE, READ, WRITE},
    runtime::WindowStep,
};
mod position;
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
pub(in crate::x64::runtime) fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "CreateWindowExA" | "CreateWindowExW" => 12,
        "DefWindowProcA" | "DefWindowProcW" | "SendMessageA" | "SendMessageW" => 4,
        "CallWindowProcA" | "CallWindowProcW" => 5,
        "SetWindowPos" => 7,
        "MoveWindow" => 6,
        "ShowWindow"
        | "GetWindowLongA"
        | "GetWindowLongW"
        | "GetWindowLongPtrA"
        | "GetWindowLongPtrW"
        | "GetWindowRect"
        | "GetClientRect"
        | "SetWindowTextA"
        | "SetWindowTextW"
        | "ClientToScreen"
        | "ScreenToClient"
        | "GetWindowThreadProcessId" => 2,
        "SetWindowLongA" | "SetWindowLongW" | "SetWindowLongPtrA" | "SetWindowLongPtrW" => 3,
        "DestroyWindow"
        | "DispatchMessageA"
        | "DispatchMessageW"
        | "IsWindow"
        | "IsWindowVisible"
        | "IsWindowUnicode"
        | "GetParent"
        | "SetFocus"
        | "SetForegroundWindow"
        | "UpdateWindow"
        | "IsIconic" => 1,
        "GetFocus" | "GetActiveWindow" | "GetForegroundWindow" => 0,
        "GetWindowTextLengthA" | "GetWindowTextLengthW" => 1,
        "GetWindowTextA" | "GetWindowTextW" => 3,
        _ => return None,
    })
}
pub(in crate::x64::runtime) fn dwords(name: &str) -> &'static [usize] {
    match name {
        "CreateWindowExA" | "CreateWindowExW" => &[0, 3, 4, 5, 6, 7],
        "DefWindowProcA" | "DefWindowProcW" | "SendMessageA" | "SendMessageW" | "ShowWindow"
        | "GetWindowLongA" | "GetWindowLongW" | "GetWindowLongPtrA" | "GetWindowLongPtrW"
        | "SetWindowLongPtrA" | "SetWindowLongPtrW" => &[1],
        "SetWindowLongA" | "SetWindowLongW" => &[1, 2],
        "SetWindowPos" => &[2, 3, 4, 5, 6],
        "MoveWindow" => &[1, 2, 3, 4, 5],
        "CallWindowProcA" | "CallWindowProcW" | "GetWindowTextA" | "GetWindowTextW" => &[2],
        _ => &[],
    }
}
pub(in crate::x64::runtime) fn is_callback(name: &str) -> bool {
    matches!(
        name,
        "CreateWindowExA"
            | "DispatchMessageA"
            | "DispatchMessageW"
            | "CreateWindowExW"
            | "SendMessageA"
            | "SendMessageW"
            | "CallWindowProcA"
            | "CallWindowProcW"
            | "DestroyWindow"
            | "ShowWindow"
            | "SetWindowPos"
            | "MoveWindow"
            | "DefWindowProcA"
            | "DefWindowProcW"
    )
}
impl Runtime64 {
    pub fn window_info(&self) -> Vec<super::super::WindowInfo> {
        self.user
            .windows
            .iter()
            .filter(|(_, w)| w.ready && w.parent == 0)
            .map(|(&handle, w)| super::super::WindowInfo {
                handle,
                title: w.title.clone(),
                width: (w.client[2] - w.client[0]).max(0) as u32,
                height: (w.client[3] - w.client[1]).max(0) as u32,
                visible: w.style & 0x10000000 != 0,
            })
            .collect()
    }
    pub fn queue_window_event(&mut self, event: super::super::WindowEvent) -> Result<(), String> {
        if !self.user.windows.contains_key(&event.window) {
            return Ok(());
        }
        if self.user.events.len() >= 4096 {
            return Err("Win64 native window event capacity exceeded".into());
        }
        let time = self.start.elapsed().as_millis() as u32;
        if !self.user.keyboard.host64([
            event.window,
            event.message as u64,
            event.wparam,
            event.lparam,
            time as u64,
        ]) {
            return Ok(());
        }
        if (0x200..=0x20e).contains(&event.message) {
            let w = &self.user.windows[&event.window];
            self.user.cursor_pos = [
                w.client[0].wrapping_add(event.lparam as u16 as i16 as i32),
                w.client[1].wrapping_add((event.lparam >> 16) as u16 as i16 as i32),
            ];
        }
        self.user.events.push_back(super::message::Message {
            event,
            time,
            tid: self.user.windows[&event.window].tid,
            point: self.user.cursor_pos,
            hardware: true,
        });
        Ok(())
    }
    pub fn take_window_events(&mut self) -> Vec<super::super::WindowEvent> {
        self.user.events.drain(..).map(|m| m.event).collect()
    }
    fn valid_procedure(&self, mem: &Memory, procedure: u64) -> Result<(), String> {
        if self.gateways.contains_key(&procedure) {
            Ok(())
        } else {
            mem.check(procedure, 1, READ | EXECUTE)
        }
    }
    pub(in crate::x64::runtime) fn window_start(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
        returned: &Cpu64,
    ) -> Result<WindowResult, String> {
        let wide = name.ends_with('W');
        if !self.threads.enabled {
            return Err("Win64 WNDPROC requires a process executor".into());
        }
        let frame = (returned.regs[4]
            .checked_sub(48)
            .ok_or("WNDPROC stack underflow")?
            & !15)
            + 8;
        mem.check(frame, 40, WRITE)?;
        if name.starts_with("DispatchMessage") {
            let words = super::message::read(mem, args[0])?;
            if words[0] == 0 {
                return Ok(WindowResult::Value(0));
            }
            if words[1] == 0x113 && words[3] != 0 {
                return Err("Win64 timer-procedure dispatch is not implemented".into());
            }
            let mut result = self.window_start(
                mem,
                if wide { "SendMessageW" } else { "SendMessageA" },
                &words[..4],
                returned,
            )?;
            if let WindowResult::Callback { call, .. } = &mut result {
                call.name = name.into();
                call.args = args.to_vec();
            }
            return Ok(result);
        }
        if name.starts_with("DefWindowProc") {
            if args[1] == 0x47 {
                return self.default_position_start(mem, name, args);
            }
            return self.window_query(mem, name, args).map(WindowResult::Value);
        }
        if matches!(name, "SetWindowPos" | "MoveWindow") {
            return self.position_start(mem, name, args);
        }
        let (target, parameters, operation, release) = if name.starts_with("Create") {
            let key = Self::resource_key(mem, args[1], wide)?;
            let class = self
                .user
                .classes
                .iter()
                .find(|(atom, class)| {
                    (class.instance == args[10] || class.style & 0x4000 != 0)
                        && match &key {
                            super::super::resources::Key::Id(id) => **atom as u32 == *id,
                            super::super::resources::Key::Name(name) => {
                                class.name.eq_ignore_ascii_case(name)
                            }
                        }
                })
                .map(|(&atom, class)| (atom, class.clone()));
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
        } else if name.starts_with("CallWindowProc") {
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
            if name == "DestroyWindow" {
                (
                    procedure,
                    [args[0], 2, 0, 0],
                    Operation::Destroy {
                        window: args[0],
                        stage: 0,
                    },
                    Vec::new(),
                )
            } else if name == "ShowWindow" {
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
    pub(in crate::x64::runtime) fn window_query(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        if name.starts_with("SetWindowLongPtr") && args[1] as u32 as i32 == -4 {
            if args[2] == 0 {
                self.set_error(mem, 87)?;
                return Ok(0);
            }
            self.valid_procedure(mem, args[2])?;
        }
        match name {
            "GetFocus" => return Ok(self.user.focus),
            "GetActiveWindow" => return Ok(self.user.active),
            "GetForegroundWindow" => return Ok(self.user.foreground),
            "IsWindow" => return Ok(self.user.windows.contains_key(&args[0]) as u64),
            _ => {}
        }
        let Some(w) = self.user.windows.get_mut(&args[0]) else {
            self.set_error(mem, 1400)?;
            return Ok(0);
        };
        match name {
            "DefWindowProcA" | "DefWindowProcW" => match args[1] {
                0x81 => Ok(1),
                0x83 => {
                    if args[2] > 1 {
                        return Err("invalid NCCALCSIZE wparam".into());
                    }
                    let mut rect = rectangle(mem, args[3])?;
                    let frame =
                        crate::user32_geometry::frame(w.style, w.ex_style, w.menu != 0, true);
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
            "GetParent" => Ok(w.parent),
            "IsWindowVisible" => Ok((w.style & 0x10000000 != 0) as u64),
            "IsWindowUnicode" => Ok(w.wide as u64),
            "IsIconic" => Ok((w.style & 0x20000000 != 0) as u64),
            "GetClientRect" => {
                write_rect(
                    mem,
                    args[1],
                    [0, 0, w.client[2] - w.client[0], w.client[3] - w.client[1]],
                )?;
                Ok(1)
            }
            "GetWindowRect" => {
                write_rect(mem, args[1], w.rect)?;
                Ok(1)
            }
            "ClientToScreen" | "ScreenToClient" => {
                let mut bytes = [0; 8];
                mem.read(args[1], &mut bytes)?;
                for i in 0..2 {
                    let value = i32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap());
                    let value = if name == "ClientToScreen" {
                        value.wrapping_add(w.client[i])
                    } else {
                        value.wrapping_sub(w.client[i])
                    };
                    bytes[4 * i..4 * i + 4].copy_from_slice(&value.to_le_bytes());
                }
                mem.write(args[1], &bytes)?;
                Ok(1)
            }
            "GetWindowThreadProcessId" => {
                if args[1] != 0 {
                    mem.write_int(args[1], 4, 1)?;
                }
                Ok(w.tid)
            }
            "SetWindowTextA" | "SetWindowTextW" => {
                let title = if args[1] == 0 {
                    String::new()
                } else {
                    Self::path_name(mem, args[1], name.ends_with('W'))?
                };
                w.title = title;
                Ok(1)
            }
            "GetWindowTextLengthA" | "GetWindowTextLengthW" => Ok(if name.ends_with('W') {
                w.title.encode_utf16().count()
            } else {
                crate::codepage::encode(&w.title, 932)
                    .map_err(|e| format!("window title: {e}"))?
                    .0
                    .len()
            } as u64),
            "GetWindowTextA" | "GetWindowTextW" => {
                if args[2] == 0 {
                    return Ok(0);
                }
                let wide = name.ends_with('W');
                let mut bytes = if wide {
                    w.title
                        .encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                } else {
                    crate::codepage::encode(&w.title, 932)
                        .map_err(|e| format!("window title: {e}"))?
                        .0
                };
                let width = if wide { 2 } else { 1 };
                bytes.truncate(bytes.len().min((args[2] as usize - 1) * width));
                let length = bytes.len() / width;
                bytes.extend(vec![0; width]);
                mem.write(args[1], &bytes)?;
                Ok(length as u64)
            }
            name if name.contains("WindowLong") => {
                let ptr = name.contains("Ptr");
                let width = if ptr { 8 } else { 4 };
                let index = args[1] as u32 as i32;
                let set = name.starts_with("Set");
                let field = match index {
                    -16 => Some(w.style as u64),
                    -20 => Some(w.ex_style as u64),
                    -4 if ptr => Some(w.procedure),
                    -6 if ptr => Some(w.instance),
                    -8 if ptr => Some(w.parent),
                    -12 if ptr => Some(w.menu),
                    -21 if ptr => Some(w.data),
                    _ => None,
                };
                let old = if let Some(field) = field {
                    field
                } else if index >= 0
                    && (index as usize)
                        .checked_add(width)
                        .is_some_and(|end| end <= w.extra.len())
                {
                    let mut bytes = [0; 8];
                    bytes[..width]
                        .copy_from_slice(&w.extra[index as usize..index as usize + width]);
                    u64::from_le_bytes(bytes)
                } else {
                    self.set_error(mem, 1413)?;
                    return Ok(0);
                };
                if set {
                    let value = args[2];
                    match index {
                        -16 => w.style = value as u32,
                        -20 => w.ex_style = value as u32,
                        -4 => w.procedure = value,
                        -6 => w.instance = value,
                        -8 => return Err("Win64 reparenting is not implemented".into()),
                        -12 => w.menu = value,
                        -21 => w.data = value,
                        _ => w.extra[index as usize..index as usize + width]
                            .copy_from_slice(&value.to_le_bytes()[..width]),
                    }
                }
                Ok(old)
            }
            "SetFocus" => {
                let previous = self.user.focus;
                self.user.focus = args[0];
                Ok(previous)
            }
            "SetForegroundWindow" => {
                self.user.foreground = args[0];
                self.user.active = args[0];
                Ok(1)
            }
            "UpdateWindow" => Ok(1), // No Win64 GDI update region has been invalidated yet.
            _ => unreachable!(),
        }
    }
}
