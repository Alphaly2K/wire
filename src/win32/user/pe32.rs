//! PE32 USER state and checked requests; guest execution belongs to Process.
use crate::api::ApiTable;
use crate::mem::GuestMem;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

mod classes;
mod control;
mod dialog;
mod host;
mod lifecycle;
mod messages;
mod objects;
mod paint;
mod queries;
mod queues;
mod sent;
mod text;
mod window_geometry;
mod window_state;
pub use classes::WindowClass;
pub(crate) use control::{ControlRequest, CreationRequest, DefaultRequest};
pub(crate) use dialog::{dialog_integer, DialogRequest};
pub(crate) use messages::MessageRequest;
pub(crate) use paint::{Operation as PaintingOperation, PaintRequest};
use queues::{clock_ms, message_bits};
pub(crate) use window_geometry::WindowRequest;
pub(crate) use window_state::WindowStateRequest;

pub struct User32 {
    pub classes: BTreeMap<u16, WindowClass>,
    pub windows: BTreeMap<u32, Window>,
    pub timers: BTreeMap<(u32, u32, u32), Timer>, // tid, HWND, id
    queues: BTreeMap<u32, VecDeque<Message>>,
    queue_changed: BTreeMap<u32, u32>,
    sent: BTreeMap<u32, SentMessage>,
    next_sent: u32,
    next_atom: u32,
    next_window: u32,
    next_timer: u32,
    pub(crate) focus: u32,
    pub(crate) active: u32,
    pub(crate) foreground: u32,
    pub(crate) capture: u32,
    pub(crate) cursor: crate::win32::user::cursor::Cursor<u32, u32>,
    keyboard: crate::keyboard::Keyboard,
    registered: crate::win32::user::objects::RegisteredNames,
}

pub struct Timer {
    pub deadline: Instant,
    period: Duration,
    proc: u32,
}

#[derive(Clone, Copy)]
struct Message {
    hwnd: u32,
    message: u32,
    wparam: u32,
    lparam: u32,
    time: u32,
    point: [i32; 2],
    synthetic_paint: bool,
    hardware_input: bool,
}

pub struct Window {
    pub class: u16,
    pub wndproc: u32,
    pub instance: u32,
    pub parent: u32,
    pub menu: u32,
    pub style: u32,
    pub ex_style: u32,
    pub rect: [i32; 4], // x, y, width, height; no platform nonclient metrics yet
    pub user_data: u32,
    pub extra: Vec<u8>,
    pub wide: bool,
    pub tid: u32,
    pub(crate) text: Vec<u16>,
    pub(crate) update: Option<[i32; 4]>,
    pub(crate) erase: bool,
    props: crate::win32::user::objects::Properties,
    destroying: bool,
}
impl Window {
    pub(crate) fn frame(&self) -> [i32; 4] {
        super::geometry::frame(
            self.style,
            self.ex_style,
            self.style & 0x40000000 == 0 && self.menu != 0,
            true,
        )
    }
    pub(crate) fn client_size(&self) -> [i32; 2] {
        let f = self.frame();
        [
            self.rect[2]
                .saturating_sub(f[0])
                .saturating_sub(f[2])
                .max(0),
            self.rect[3]
                .saturating_sub(f[1])
                .saturating_sub(f[3])
                .max(0),
        ]
    }
}

impl Default for User32 {
    fn default() -> Self {
        Self {
            classes: BTreeMap::new(),
            windows: BTreeMap::new(),
            timers: BTreeMap::new(),
            queues: BTreeMap::new(),
            queue_changed: BTreeMap::new(),
            sent: BTreeMap::new(),
            next_sent: 1,
            next_atom: 0xc000,
            next_window: 0x4001_0000,
            next_timer: 1,
            focus: 0,
            active: 0,
            foreground: 0,
            capture: 0,
            cursor: Default::default(),
            keyboard: Default::default(),
            registered: Default::default(),
        }
    }
}
struct SentMessage {
    sender: u32,
    receiver: u32,
    args: [u32; 4],
    processing: bool,
    result: Option<u32>,
}

impl User32 {
    pub(crate) fn keyboard_call(
        &mut self,
        id: crate::win32::catalog::KeyboardApi,
        args: &crate::api::Args,
        mem: &mut GuestMem,
        tid: u32,
    ) -> Result<u32, String> {
        crate::win32::x86::keyboard::call(id, args, mem, &mut self.keyboard, tid)
    }
}
