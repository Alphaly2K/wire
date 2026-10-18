//! Checked Win64 USER32 objects. Guest class pointers never become host pointers.
use super::{Memory, Runtime64, READ};
use crate::win32::x64::geometry::{read_rect as rectangle, write_rect};
mod classes;
mod cursor;
mod desktop;
mod gdi;
mod images;
mod message;
mod window;
pub(super) use message::arity as message_arity;
use std::collections::BTreeMap;
pub use window::WindowCall;
pub(super) use window::WindowResult;
pub(super) struct Window {
    class: u16,
    procedure: u64,
    instance: u64,
    parent: u64,
    menu: u64,
    style: u32,
    ex_style: u32,
    pub(super) rect: [i32; 4],
    client: [i32; 4],
    data: u64,
    extra: Vec<u8>,
    tid: u64,
    wide: bool,
    title: String,
    ready: bool,
}
pub(super) struct User {
    desktop: crate::win32::user::desktop::Settings,
    images: crate::win32::user::images::Images,
    classes: crate::win32::user::classes64::Classes,
    cursor: crate::win32::x64::cursor::Cursor64,
    pub(super) windows: BTreeMap<u64, Window>,
    next_window: u64,
    focus: u64,
    active: u64,
    foreground: u64,
    dcs: crate::win32::gdi::dc::DeviceContexts,
    pub(super) events: std::collections::VecDeque<message::Message>,
    pub(super) keyboard: crate::keyboard::Keyboard,
    message_state: BTreeMap<u64, [u64; 3]>,
}
impl Default for User {
    fn default() -> Self {
        Self {
            desktop: Default::default(),
            images: crate::win32::user::images::Images::new(
                super::TEB + 0x610000,
                super::TEB + 0x620000,
            ),
            classes: Default::default(),
            cursor: crate::win32::x64::cursor::Cursor64::new([
                0,
                0,
                super::display::WIDTH as i32,
                super::display::HEIGHT as i32,
            ]),
            windows: BTreeMap::new(),
            next_window: super::TEB + 0x630000,
            focus: 0,
            active: 0,
            foreground: 0,
            dcs: crate::win32::gdi::dc::DeviceContexts::new(
                super::TEB + 0x650000,
                super::TEB + 0x660000,
            ),
            events: std::collections::VecDeque::new(),
            keyboard: Default::default(),
            message_state: BTreeMap::new(),
        }
    }
}
pub(super) fn arity(name: &str) -> Option<usize> {
    message::arity(name).or_else(|| window::arity(name))
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    if message::arity(name).is_some() {
        return message::dwords(name);
    }
    if window::arity(name).is_some() {
        return window::dwords(name);
    }
    &[]
}
impl Runtime64 {
    pub(super) fn user_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        self.window_query(mem, name, args)
    }
}
pub(super) fn is_callback(name: &str) -> bool {
    window::is_callback(name)
}
