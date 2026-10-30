//! USER metadata and checked scalar operations; WNDPROC execution lives in Process.
use super::{
    catalog::{ApiId, ClassApi as C, WindowControlApi as W, WindowQueryApi as Q},
    context, modules, ServiceError, Services,
};
use crate::{
    machine::{Mode, State},
    memory::{Memory, EXECUTE, READ},
};
use std::collections::{BTreeMap, BTreeSet};
#[path = "../../../../src/win32/user/class_metadata.rs"]
pub(crate) mod class_metadata;
#[path = "../../../../src/win32/user/classes64.rs"]
pub(crate) mod classes;
#[path = "../../../../src/win32/user/dialog_values.rs"]
pub(crate) mod dialog_values;
#[allow(dead_code)]
#[path = "../../../../src/win32/user/geometry.rs"]
pub(crate) mod geometry;
#[path = "../../../../src/win32/user/hierarchy.rs"]
pub(crate) mod hierarchy;
#[allow(dead_code)]
#[path = "../../../../src/win32/user/windows64.rs"]
pub(crate) mod records;
pub(crate) use records::Window;
#[path = "../../../../src/win32/user/cursor.rs"]
pub(crate) mod cursor;
#[path = "../../../../src/win32/user/image_services.rs"]
pub(crate) mod image_services;
#[allow(dead_code)]
#[path = "../../../../src/win32/user/images.rs"]
pub(crate) mod images;
#[allow(dead_code)]
#[path = "../../../../src/win32/user/input_profile.rs"]
pub(crate) mod input_profile;
#[path = "../../../../src/win32/user/menus.rs"]
pub(crate) mod menus;
#[path = "../../../../src/win32/user/mouse_pointer.rs"]
pub(crate) mod mouse_pointer;
#[path = "../../../../src/win32/user/object_services.rs"]
pub(crate) mod object_services;
#[path = "../../../../src/win32/user/objects.rs"]
pub(crate) mod objects;
#[path = "../../../../src/win32/user/query_values.rs"]
pub(crate) mod query_values;
#[path = "../../../../src/win32/user/raw_input.rs"]
pub(crate) mod raw_input;
#[path = "../../../../src/win32/user/raw_input_calls.rs"]
pub(crate) mod raw_input_calls;
#[path = "../../../../src/win32/user/resources.rs"]
pub(crate) mod resources;

#[derive(Default)]
pub(crate) struct Input {
    pub focus: u64,
    pub active: u64,
    pub capture: u64,
}
pub(crate) struct User {
    pub mode: Mode,
    pub menus: menus::Menus,
    pub classes: classes::Classes,
    pub class_metadata: BTreeMap<u16, class_metadata::Record>,
    pub windows: BTreeMap<u64, Window>,
    pub destroying: BTreeSet<u64>,
    pub next_window: u64,
    pub messages: super::messages::Queue,
    pub keyboard: crate::keyboard::Keyboard,
    pub desktop: super::display::desktop::Settings,
    pub devices: super::device_state::Devices,
    pub cursor: cursor::Cursor<u64, u64>,
    pub images: images::Images,
    pub clip: Option<[i32; 4]>,
    pub mouse_pointer: mouse_pointer::MousePointer,
    pub raw_input: raw_input::RawInput,
    pub input: BTreeMap<u32, Input>,
    pub foreground: u64,
    pub gdi: super::gdi::Gdi,
    pub paints: BTreeMap<u64, super::painting::Paint>,
    pub class_brushes: BTreeMap<u16, u64>,
    pub registered: objects::RegisteredNames,
    pub properties: BTreeMap<u64, objects::Properties>,
}
impl User {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            menus: menus::Menus::with_limits(0x42000000, 16 * 1024 * 1024),
            classes: classes::Classes::new(mode == Mode::X86),
            class_metadata: BTreeMap::new(),
            windows: BTreeMap::new(),
            destroying: BTreeSet::new(),
            messages: Default::default(),
            keyboard: Default::default(),
            desktop: Default::default(),
            devices: super::device_state::Devices::new(0x7ffd00680000, 0x7ffd006a0000),
            cursor: Default::default(),
            images: images::Images::with_byte_budget(
                if mode == Mode::X86 {
                    0x44000000
                } else {
                    0x7ffd00610000
                },
                if mode == Mode::X86 {
                    0x44010000
                } else {
                    0x7ffd00620000
                },
                Some(16 * 1024 * 1024),
            ),
            clip: None,
            mouse_pointer: Default::default(),
            raw_input: Default::default(),
            input: BTreeMap::new(),
            foreground: 0,
            gdi: super::gdi::Gdi::new(mode),
            paints: BTreeMap::new(),
            class_brushes: BTreeMap::new(),
            registered: Default::default(),
            properties: BTreeMap::new(),
            next_window: if mode == Mode::X86 {
                0x40010000
            } else {
                0x7ffd00630000
            },
        }
    }
    pub fn remove(&mut self, handle: u64) {
        let mut children =
            hierarchy::children(self.windows.iter().map(|(&h, w)| (h, w.link())), handle);
        children.reverse();
        children.push(handle);
        for h in children {
            self.remove_one(h);
        }
    }
    fn remove_one(&mut self, handle: u64) {
        let removed = self.windows.remove(&handle);
        self.properties.remove(&handle);
        self.devices.remove_notifications(handle);
        self.mouse_pointer.remove_window(handle);
        self.raw_input.remove_window(handle);
        for input in self.input.values_mut() {
            if input.focus == handle {
                input.focus = 0;
            }
            if input.active == handle {
                input.active = 0;
            }
            if input.capture == handle {
                input.capture = 0;
            }
        }
        if self.foreground == handle {
            self.foreground = 0;
        }

        self.destroying.remove(&handle);
        self.messages.remove_window(handle);
        self.gdi.remove_window(handle);
        self.paints.remove(&handle);
        self.keyboard.remove_window64(handle);
        if self.mode == Mode::X86 {
            if let Some(window) = removed.filter(|w| w.style & 0x40000000 == 0 && w.menu != 0) {
                self.menus.destroy(window.menu as u32);
                super::menus::retire_windows(self);
            }
        }
    }
    pub fn exit_thread(&mut self, id: u32) {
        let windows: Vec<_> = self
            .windows
            .iter()
            .filter(|(_, w)| w.tid == u64::from(id))
            .map(|(&h, _)| h)
            .collect();
        for handle in windows {
            self.remove(handle);
        }
        self.messages.exit(id);
        self.keyboard.exit64(u64::from(id));
        self.input.remove(&id);
        self.cursor.exit(u64::from(id));
        self.mouse_pointer.current.remove(&u64::from(id));
        self.gdi.exit_thread(id);
    }
    pub fn window_info(&self) -> Vec<records::WindowInfo> {
        self.windows
            .iter()
            .filter(|(h, w)| {
                w.ready
                    && w.style & 0x40000000 == 0
                    && !hierarchy::message_parent(w.parent)
                    && **h != u64::from(query_values::DESKTOP)
            })
            .map(|(&handle, w)| records::WindowInfo {
                handle,
                title: w.title.clone(),
                width: (w.client[2] - w.client[0]).max(0) as u32,
                height: (w.client[3] - w.client[1]).max(0) as u32,
                visible: w.style & 0x10000000 != 0,
                topmost: w.ex_style & 8 != 0,
            })
            .collect()
    }
    pub(crate) fn client_origin(&self, handle: u64) -> Result<[i32; 2], u32> {
        hierarchy::origin(handle, |h| self.windows.get(&h).map(Window::link)).map_err(|_| 1400)
    }
    pub(crate) fn window_rect(&self, handle: u64) -> Result<[i32; 4], u32> {
        let w = self.windows.get(&handle).ok_or(1400u32)?;
        let origin = if w.style & 0x40000000 != 0 && !hierarchy::message_parent(w.parent) {
            self.client_origin(w.parent)?
        } else {
            [0; 2]
        };
        Ok(geometry::offset(w.rect, origin[0], origin[1]))
    }
}
pub(super) fn implemented(id: ApiId) -> bool {
    matches!(
        id,
        ApiId::Class(_)
            | ApiId::Keyboard(_)
            | ApiId::WindowState(_)
            | ApiId::Painting(_)
            | ApiId::WindowControl(_)
            | ApiId::Dialog(_)
            | ApiId::WindowQuery(_)
    )
}
pub(crate) fn name(memory: &Memory, address: u64, wide: bool) -> Result<String, ServiceError> {
    modules::name(memory, address, wide).map_err(|e| match e {
        modules::Failure::Fault(f) => ServiceError::Fault(f),
        _ => ServiceError::Unavailable("invalid or overlong USER string"),
    })
}
pub(crate) fn error(memory: &mut Memory, state: &State, code: u32) -> Result<u64, ServiceError> {
    memory.write_int(
        context::error_address(state, memory.mode()),
        4,
        u64::from(code),
    )?;
    Ok(0)
}
pub(crate) fn procedure(
    memory: &Memory,
    services: &Services,
    target: u64,
) -> Result<(), ServiceError> {
    if !services.is_bound(target) {
        memory.check(target, 1, READ | EXECUTE)?;
    }
    Ok(())
}
pub(crate) fn rect(memory: &Memory, address: u64) -> Result<[i32; 4], crate::memory::Fault> {
    let mut bytes = [0; 16];
    memory.read(address, &mut bytes)?;
    Ok(std::array::from_fn(|n| {
        i32::from_le_bytes(bytes[n * 4..n * 4 + 4].try_into().unwrap())
    }))
}
pub(crate) fn write_rect(
    memory: &mut Memory,
    address: u64,
    rect: [i32; 4],
) -> Result<(), crate::memory::Fault> {
    memory.write(
        address,
        &rect
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
}
impl User {
    pub fn class_call64(
        &mut self,
        id: C,
        a: &[u64; 16],
        state: &State,
        memory: &mut Memory,
    ) -> Result<u64, ServiceError> {
        debug_assert_eq!(memory.mode(), Mode::X64);
        if id.register() {
            let ex = id.extended();
            let pointer = a[0];
            let length = if ex { 80 } else { 72 };
            memory.check(pointer, length, READ)?;
            if ex && memory.read_int(pointer, 4)? != length as u64 {
                return error(memory, state, 87);
            }
            let style = memory.read_int(pointer + if ex { 4 } else { 0 }, 4)?;
            let proc = memory.read_int(pointer + 8, 8)?;
            let ce = memory.read_int(pointer + 16, 4)?;
            let we = memory.read_int(pointer + 20, 4)?;
            let instance = memory.read_int(pointer + 24, 8)?;
            let menu = memory.read_int(pointer + 56, 8)?;
            let class = memory.read_int(pointer + 64, 8)?;
            if class <= 65535 || proc == 0 || ce > 4096 || we > 4096 {
                return error(memory, state, 87);
            }
            let text = name(memory, class, id.wide())?;
            if text.is_empty() || text.len() > 256 {
                return error(memory, state, 87);
            }
            if ce != 0 || menu != 0 {
                return error(memory, state, 50);
            }
            let brush = memory.read_int(pointer + 48, 8)?;
            return match self.classes.register(classes::Class {
                instance,
                name: text,
                procedure: proc,
                style: style as u32,
                window_extra: we as usize,
                wide: id.wide(),
            }) {
                Ok(atom) => {
                    self.class_brushes.insert(atom, brush);
                    Ok(u64::from(atom))
                }
                Err(code) => error(memory, state, code),
            };
        }
        let text = if a[0] > 65535 {
            Some(name(memory, a[0], id.wide())?)
        } else {
            None
        };
        let key = text
            .as_deref()
            .map_or(classes::Key::Id(a[0] as u32), classes::Key::Name);
        let Some(atom) = self.classes.find(a[1], key, false).map(|(atom, _)| atom) else {
            return error(memory, state, 1411);
        };
        if self.windows.values().any(|w| w.class == atom) {
            return error(memory, state, 1412);
        }
        self.classes.remove(atom);
        self.class_brushes.remove(&atom);
        Ok(1)
    }
    pub fn query(
        &mut self,
        id: Q,
        a: &[u64; 16],
        state: &State,
        memory: &mut Memory,
        services: &Services,
    ) -> Result<u64, ServiceError> {
        let pointer = id.pointer() || memory.mode() == Mode::X86;
        if id == Q::Related {
            let Some(window) = self.windows.get(&a[0]) else {
                return error(memory, state, 1400);
            };
            if a[1] == 4 {
                return Ok(
                    if window.style & 0x40000000 == 0 && !hierarchy::message_parent(window.parent) {
                        window.parent
                    } else {
                        0
                    },
                );
            }
            if a[1] == 6 {
                return error(memory, state, 50);
            }
            if a[1] > 6 {
                return error(memory, state, 87);
            }
            let child = window.style & 0x40000000 != 0;
            let siblings: Vec<_> = self
                .windows
                .iter()
                .rev()
                .filter(|(_, w)| {
                    if a[1] == 5 {
                        if window.tid == 0 {
                            w.tid != 0
                                && w.style & 0x40000000 == 0
                                && !hierarchy::message_parent(w.parent)
                        } else {
                            w.parent == a[0] && w.style & 0x40000000 != 0
                        }
                    } else {
                        (w.style & 0x40000000 != 0) == child
                            && (if child {
                                w.parent == window.parent
                            } else {
                                !hierarchy::message_parent(w.parent) && w.tid != 0
                            })
                    }
                })
                .map(|(&h, _)| h)
                .collect();
            return Ok(match a[1] {
                0 | 5 => siblings.first().copied(),
                1 => siblings.last().copied(),
                2 | 3 => siblings
                    .iter()
                    .position(|&h| h == a[0])
                    .and_then(|n| {
                        if a[1] == 2 {
                            n.checked_add(1)
                        } else {
                            n.checked_sub(1)
                        }
                    })
                    .and_then(|n| siblings.get(n).copied()),
                _ => None,
            }
            .unwrap_or(0));
        }
        if matches!(id, Q::FindA | Q::FindW) {
            let class = if a[0] > 65535 {
                Some(super::class_services::window_class_name(
                    memory,
                    a[0],
                    id == Q::FindW,
                )?)
            } else {
                None
            };
            let title = if a[1] != 0 {
                Some(super::class_services::window_class_name(
                    memory,
                    a[1],
                    id == Q::FindW,
                )?)
            } else {
                None
            };
            let search = query_values::Search::new(a[0] as u32, class, title);
            return Ok(self
                .windows
                .iter()
                .find_map(|(&h, w)| {
                    let class = self.class_metadata.get(&w.class)?;
                    search
                        .matches(w.style, w.parent, w.class, &class.name, &w.title)
                        .then_some(h)
                })
                .unwrap_or(0));
        }
        if id == Q::MapPoints {
            if a[3] > 4096 {
                return error(memory, state, 87);
            }
            let origin = |h| self.client_origin(h).ok();
            let (Some(from), Some(to)) = (origin(a[0]), origin(a[1])) else {
                return error(memory, state, 1400);
            };
            let mut bytes = vec![0; a[3] as usize * 8];
            memory.read(a[2], &mut bytes)?;
            let result = query_values::map_points(
                &mut bytes,
                [from[0].wrapping_sub(to[0]), from[1].wrapping_sub(to[1])],
            );
            memory.write(a[2], &bytes)?;
            return Ok(result.into());
        }
        if id == Q::Placement && memory.mode() == Mode::X86 {
            return error(memory, state, 120);
        }
        if matches!(id, Q::Zoomed | Q::Enabled) && !self.windows.contains_key(&a[0]) {
            return Ok(0);
        }
        if id.setting() && pointer && a[1] as u32 as i32 == -4 {
            if a[2] == 0 {
                return error(memory, state, 87);
            }
            procedure(memory, services, a[2])?;
        }
        if id == Q::IsWindow {
            return Ok(u64::from(self.windows.contains_key(&a[0])));
        }
        let origin = if matches!(id, Q::ClientToScreen | Q::ScreenToClient) {
            match self.client_origin(a[0]) {
                Ok(origin) => origin,
                Err(code) => return error(memory, state, code),
            }
        } else {
            [0; 2]
        };
        let screen_rect = if id == Q::WindowRect {
            match self.window_rect(a[0]) {
                Ok(rect) => rect,
                Err(code) => return error(memory, state, code),
            }
        } else {
            [0; 4]
        };
        let Some(w) = self.windows.get_mut(&a[0]) else {
            return error(memory, state, 1400);
        };
        Ok(match id {
            Q::Parent => w.parent,
            Q::Visible => u64::from(w.style & 0x10000000 != 0),
            Q::Unicode => u64::from(w.wide),
            Q::Iconic => u64::from(w.style & 0x20000000 != 0),
            Q::Zoomed => u64::from(w.style & 0x01000000 != 0),
            Q::Enabled => u64::from(w.style & 0x08000000 == 0),
            Q::Placement => {
                if a[1] == 0 || memory.read_int(a[1], 4)? != 44 {
                    return error(memory, state, 87);
                }
                memory.check(a[1], 44, crate::memory::WRITE)?;
                if w.style & 0x21000000 != 0 {
                    return error(memory, state, 50);
                }
                memory.write(a[1], &query_values::placement(w.rect))?;
                1
            }
            Q::ThreadProcess => {
                if a[1] != 0 {
                    memory.write_int(a[1], 4, 1)?;
                }
                w.tid
            }
            Q::ClientRect => {
                write_rect(
                    memory,
                    a[1],
                    [0, 0, w.client[2] - w.client[0], w.client[3] - w.client[1]],
                )?;
                1
            }
            Q::WindowRect => {
                write_rect(memory, a[1], screen_rect)?;
                1
            }
            Q::ClientToScreen | Q::ScreenToClient => {
                let mut b = [0; 8];
                memory.read(a[1], &mut b)?;
                for n in 0..2 {
                    let value = i32::from_le_bytes(b[n * 4..n * 4 + 4].try_into().unwrap());
                    let value = if id == Q::ClientToScreen {
                        value.wrapping_add(origin[n])
                    } else {
                        value.wrapping_sub(origin[n])
                    };
                    b[n * 4..n * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
                memory.write(a[1], &b)?;
                1
            }
            _ => {
                let width = if id.pointer() { 8 } else { 4 };
                let index = a[1] as u32 as i32;
                let old = match index {
                    -16 => u64::from(w.style),
                    -20 => u64::from(w.ex_style),
                    -4 if pointer => w.procedure,
                    -6 if pointer => w.instance,
                    -8 if pointer => w.parent,
                    -12 if pointer => w.menu,
                    -21 if pointer => w.data,
                    n if n >= 0
                        && (n as usize)
                            .checked_add(width)
                            .is_some_and(|end| end <= w.extra.len()) =>
                    {
                        let mut bytes = [0; 8];
                        bytes[..width].copy_from_slice(&w.extra[n as usize..n as usize + width]);
                        u64::from_le_bytes(bytes)
                    }
                    _ => return error(memory, state, 1413),
                };
                if id.setting() {
                    match index {
                        -16 => w.style = a[2] as u32,
                        -20 => w.ex_style = a[2] as u32,
                        -4 => w.procedure = a[2],
                        -6 => w.instance = a[2],
                        -8 => return error(memory, state, 50),
                        -12 => w.menu = a[2],
                        -21 => w.data = a[2],
                        n => w.extra[n as usize..n as usize + width]
                            .copy_from_slice(&a[2].to_le_bytes()[..width]),
                    }
                }
                old
            }
        })
    }
    pub fn default_call(
        &mut self,
        id: W,
        a: &[u64; 16],
        state: &State,
        memory: &mut Memory,
        tid: u32,
    ) -> Result<u64, ServiceError> {
        let Some(w) = self.windows.get_mut(&a[0]) else {
            return error(memory, state, 1400);
        };
        Ok(match a[1] as u32 {
            0xff => {
                if self
                    .raw_input
                    .packets
                    .get(&a[3])
                    .is_some_and(|p| p.window == a[0])
                {
                    let packet = self.raw_input.packets.remove(&a[3]).unwrap();
                    self.messages.remove_raw(&[(a[3], packet)]);
                }
                0
            }
            0x81 => 1,
            0x14 => match self.gdi.erase(
                a[0],
                a[2],
                tid,
                self.class_brushes.get(&w.class).copied().unwrap_or(0),
            ) {
                Ok(v) => v,
                Err(e) => return error(memory, state, e),
            },
            0x83 => {
                if a[2] > 1 {
                    return error(memory, state, 87);
                }
                let mut r = rect(memory, a[3])?;
                let f = geometry::frame(
                    w.style,
                    w.ex_style,
                    w.style & 0x40000000 == 0 && w.menu != 0,
                    true,
                );
                for n in 0..4 {
                    r[n] = if n < 2 {
                        r[n].saturating_add(f[n])
                    } else {
                        r[n].saturating_sub(f[n])
                    };
                }
                r[2] = r[2].max(r[0]);
                r[3] = r[3].max(r[1]);
                write_rect(memory, a[3], r)?;
                0
            }
            0x0c => {
                w.title = if a[3] == 0 {
                    String::new()
                } else {
                    name(memory, a[3], id.wide())?
                };
                1
            }
            0x0d | 0x0e => {
                let mut bytes = if id.wide() {
                    w.title
                        .encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>()
                } else {
                    crate::codepage::encode(&w.title, 932)
                        .map_err(|_| ServiceError::Unavailable("invalid USER title encoding"))?
                        .0
                };
                let unit = if id.wide() { 2 } else { 1 };
                if a[1] == 0x0e {
                    (bytes.len() / unit) as u64
                } else if a[2] == 0 {
                    0
                } else {
                    let length =
                        a[2].saturating_sub(1).min((bytes.len() / unit) as u64) as usize * unit;
                    bytes.truncate(length);
                    bytes.resize(length + unit, 0);
                    memory.write(a[3], &bytes)?;
                    (length / unit) as u64
                }
            }
            _ => 0,
        })
    }
}
