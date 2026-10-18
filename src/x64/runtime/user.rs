//! Checked Win64 USER32 objects. Guest class pointers never become host pointers.
use super::{Memory, Runtime64, READ};
use crate::win32::x64::geometry::{read_rect as rectangle, write_rect};
mod cursor;
mod gdi;
mod images;
mod message;
mod window;
pub(super) use message::arity as message_arity;
use std::collections::BTreeMap;
pub use window::WindowCall;
pub(super) use window::WindowResult;
#[derive(Clone)]
struct Class {
    instance: u64,
    name: String,
    procedure: u64,
    style: u32,
    window_extra: usize,
}
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
    images: crate::win32::user::images::Images,
    classes: BTreeMap<u16, Class>,
    next_atom: u16,
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
            images: crate::win32::user::images::Images::new(
                super::TEB + 0x610000,
                super::TEB + 0x620000,
            ),
            classes: BTreeMap::new(),
            next_atom: 0xc000,
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
    if let Some(arity) = message::arity(name) {
        return Some(arity);
    }
    if let Some(arity) = window::arity(name) {
        return Some(arity);
    }
    Some(match name {
        "UnregisterClassA" | "UnregisterClassW" => 2,
        "RegisterClassA" | "RegisterClassW" | "RegisterClassExA" | "RegisterClassExW" => 1,
        "DwmGetWindowAttribute" => 4,
        "DwmIsCompositionEnabled" => 1,
        _ => return None,
    })
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    if message::arity(name).is_some() {
        return message::dwords(name);
    }
    if window::arity(name).is_some() {
        return window::dwords(name);
    }
    match name {
        "DwmGetWindowAttribute" => &[1, 3],
        _ => &[],
    }
}
impl Runtime64 {
    pub(super) fn user_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        let wide = name.ends_with('W');
        if window::arity(name).is_some() {
            return self.window_query(mem, name, args);
        }
        match name {
            "DwmIsCompositionEnabled" => {
                if args[0] == 0 {
                    return Ok(0x80070057);
                }
                mem.write_int(args[0], 4, 0)?; // Guest desktop has no DWM compositor.
                Ok(0)
            }
            "DwmGetWindowAttribute" => {
                let Some(w) = self.user.windows.get(&args[0]) else {
                    return Ok(0x80070006);
                };
                if args[2] == 0 {
                    return Ok(0x80070057);
                }
                match (args[1], args[3]) {
                    (9, 16) => {
                        write_rect(mem, args[2], w.rect)?;
                        Ok(0)
                    } // Extended frame bounds.
                    (14, 4) => {
                        mem.write_int(args[2], 4, 0)?;
                        Ok(0)
                    } // No cloaking service.
                    _ => Ok(0x80070057),
                }
            }
            "RegisterClassA" | "RegisterClassW" | "RegisterClassExA" | "RegisterClassExW" => {
                let ex = name.contains("Ex");
                let pointer = args[0];
                mem.check(pointer, if ex { 80 } else { 72 }, READ)?;
                if ex && mem.read_int(pointer, 4)? != 80 {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                let style = mem.read_int(pointer + if ex { 4 } else { 0 }, 4)? as u32;
                let procedure = mem.read_int(pointer + 8, 8)?;
                let class_extra = mem.read_int(pointer + 16, 4)?;
                let window_extra = mem.read_int(pointer + 20, 4)?;
                let instance = mem.read_int(pointer + 24, 8)?;
                let class = Self::path_name(mem, mem.read_int(pointer + 64, 8)?, wide)?;
                if class.is_empty()
                    || class.len() > 256
                    || class_extra > 4096
                    || window_extra > 4096
                    || procedure == 0
                {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                if class_extra != 0 || mem.read_int(pointer + 56, 8)? != 0 {
                    self.set_error(mem, 50)?;
                    return Ok(0);
                }
                if self
                    .user
                    .classes
                    .values()
                    .any(|c| c.instance == instance && c.name.eq_ignore_ascii_case(&class))
                {
                    self.set_error(mem, 1410)?;
                    return Ok(0);
                }
                if self.user.next_atom == 0xffff {
                    self.set_error(mem, 8)?;
                    return Ok(0);
                }
                let atom = self.user.next_atom;
                self.user.next_atom += 1;
                self.user.classes.insert(
                    atom,
                    Class {
                        instance,
                        name: class,
                        procedure,
                        style,
                        window_extra: window_extra as usize,
                    },
                );
                Ok(atom as u64)
            }
            "UnregisterClassA" | "UnregisterClassW" => {
                let key = Self::resource_key(mem, args[0], wide)?;
                let atom = self
                    .user
                    .classes
                    .iter()
                    .find(|(atom, class)| {
                        class.instance == args[1]
                            && match &key {
                                super::resources::Key::Id(id) => **atom as u32 == *id,
                                super::resources::Key::Name(name) => {
                                    class.name.eq_ignore_ascii_case(name)
                                }
                            }
                    })
                    .map(|(&atom, _)| atom);
                if let Some(atom) = atom {
                    if self.user.windows.values().any(|w| w.class == atom) {
                        self.set_error(mem, 1412)?;
                        return Ok(0);
                    }
                    self.user.classes.remove(&atom);
                    Ok(1)
                } else {
                    self.set_error(mem, 1411)?;
                    Ok(0)
                }
            }
            _ => unreachable!(),
        }
    }
}
pub(super) fn is_callback(name: &str) -> bool {
    window::is_callback(name)
}
