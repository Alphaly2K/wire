//! Checked Win64 USER32 objects. Guest class pointers never become host pointers.
use super::{Memory, Runtime64, READ};
use crate::win32::x64::geometry::{read_rect as rectangle, write_rect};
mod gdi;
mod message;
mod window;
pub(super) use message::arity as message_arity;
use std::collections::BTreeMap;
pub use window::WindowCall;
pub(super) use window::WindowResult;
struct Image {
    instance: u64,
    key: super::resources::Key,
    kind: u32,
    shared: bool,
    _payload: Vec<u8>,
}
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
    images: BTreeMap<u64, Image>,
    next_image: u64,
    classes: BTreeMap<u16, Class>,
    next_atom: u16,
    cursor: u64,
    cursor_counts: BTreeMap<u64, i32>,
    pub(super) windows: BTreeMap<u64, Window>,
    next_window: u64,
    focus: u64,
    active: u64,
    foreground: u64,
    dcs: BTreeMap<u64, (u64, u64, bool)>,
    next_dc: u64,
    pub(super) events: std::collections::VecDeque<message::Message>,
    pub(super) keyboard: crate::keyboard::Keyboard,
    cursor_pos: [i32; 2],
    cursor_clip: [i32; 4],
    message_state: BTreeMap<u64, [u64; 3]>,
}
impl Default for User {
    fn default() -> Self {
        Self {
            images: BTreeMap::new(),
            next_image: super::TEB + 0x610000,
            classes: BTreeMap::new(),
            next_atom: 0xc000,
            cursor: 0,
            cursor_counts: BTreeMap::new(),
            windows: BTreeMap::new(),
            next_window: super::TEB + 0x630000,
            focus: 0,
            active: 0,
            foreground: 0,
            dcs: BTreeMap::new(),
            next_dc: super::TEB + 0x650000,
            events: std::collections::VecDeque::new(),
            keyboard: Default::default(),
            cursor_pos: [0; 2],
            cursor_clip: [
                0,
                0,
                super::display::WIDTH as i32,
                super::display::HEIGHT as i32,
            ],
            message_state: BTreeMap::new(),
        }
    }
}
pub(super) fn arity(name: &str) -> Option<usize> {
    if let Some(arity) = message::arity(name) {
        return Some(arity);
    }
    if let Some(arity) = gdi::arity(name) {
        return Some(arity);
    }
    if let Some(arity) = window::arity(name) {
        return Some(arity);
    }
    Some(match name {
        "LoadIconA" | "LoadIconW" | "LoadCursorA" | "LoadCursorW" | "UnregisterClassA"
        | "UnregisterClassW" => 2,
        "LoadImageA" | "LoadImageW" => 6,
        "DestroyIcon"
        | "DestroyCursor"
        | "SetCursor"
        | "ShowCursor"
        | "RegisterClassA"
        | "RegisterClassW"
        | "RegisterClassExA"
        | "RegisterClassExW"
        | "EnableMouseInPointer" => 1,
        "GetCursor" | "IsMouseInPointerEnabled" => 0,
        "ClipCursor" | "GetClipCursor" | "GetCursorPos" | "TrackMouseEvent" => 1,
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
        "GetDeviceCaps" => &[1],
        "DwmGetWindowAttribute" => &[1, 3],
        "ShowCursor" | "EnableMouseInPointer" => &[0],
        "LoadImageA" | "LoadImageW" => &[2, 3, 4, 5],
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
        if gdi::arity(name).is_some() {
            return self.gdi_call(mem, name, args);
        }
        if window::arity(name).is_some() {
            return self.window_query(mem, name, args);
        }
        match name {
            "TrackMouseEvent" => {
                // TRACKMOUSEEVENT is 24 bytes on Win64 (including tail padding).
                // The current AppKit bridge has no hover/leave tracking source.
                // Query/cancel the actual empty registration state; registration
                // must fail rather than promise notifications we cannot deliver.
                let mut data = [0; 24];
                mem.read(args[0], &mut data)?;
                let size = u32::from_le_bytes(data[..4].try_into().unwrap());
                let flags = u32::from_le_bytes(data[4..8].try_into().unwrap());
                if size != 24 {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                if flags & 0x40000000 != 0 {
                    let mut output = [0; 24];
                    output[..4].copy_from_slice(&24u32.to_le_bytes());
                    mem.write(args[0], &output)?;
                    return Ok(1);
                }
                let window = u64::from_le_bytes(data[8..16].try_into().unwrap());
                if !self.user.windows.contains_key(&window) {
                    self.set_error(mem, 1400)?;
                    return Ok(0);
                }
                if flags & !(0x80000000 | 0x10 | 3) != 0 {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                if flags & 0x80000000 != 0 || flags & 3 == 0 {
                    return Ok(1); // Nothing was armed; cancellation needs no backend.
                }
                self.set_error(mem, 50)?;
                Ok(0)
            }
            "GetClipCursor" => {
                write_rect(mem, args[0], self.user.cursor_clip)?;
                Ok(1)
            }
            "GetCursorPos" => {
                mem.write(
                    args[0],
                    &self
                        .user
                        .cursor_pos
                        .into_iter()
                        .flat_map(i32::to_le_bytes)
                        .collect::<Vec<_>>(),
                )?;
                Ok(1)
            }
            "ClipCursor" => {
                let screen = [
                    0,
                    0,
                    super::display::WIDTH as i32,
                    super::display::HEIGHT as i32,
                ];
                let mut clip = if args[0] == 0 {
                    screen
                } else {
                    rectangle(mem, args[0])?
                };
                clip[0] = clip[0].max(screen[0]);
                clip[1] = clip[1].max(screen[1]);
                clip[2] = clip[2].min(screen[2]);
                clip[3] = clip[3].min(screen[3]);
                if clip[0] >= clip[2] || clip[1] >= clip[3] {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                self.user.cursor_clip = clip;
                for n in 0..2 {
                    self.user.cursor_pos[n] =
                        self.user.cursor_pos[n].clamp(clip[n], clip[n + 2] - 1);
                }
                // Constrain the virtual desktop pointer, never the host desktop.
                Ok(1)
            }
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
            "LoadIconA" | "LoadIconW" | "LoadCursorA" | "LoadCursorW" | "LoadImageA"
            | "LoadImageW" => {
                let load_image = name.starts_with("LoadImage");
                let kind = if load_image {
                    args[2] as u32
                } else {
                    if name.starts_with("LoadCursor") {
                        2
                    } else {
                        1
                    }
                };
                let shared = !load_image || args[5] & 0x8000 != 0;
                if load_image && (args[5] & !0x8040 != 0 || args[3] != 0 || args[4] != 0) {
                    self.set_error(mem, 50)?;
                    return Ok(0);
                }
                if !matches!(kind, 1 | 2) {
                    self.set_error(mem, 50)?;
                    return Ok(0);
                }
                let key = Self::resource_key(mem, args[1], wide)?;
                if shared {
                    if let Some((&handle, _)) = self.user.images.iter().find(|(_, image)| {
                        image.shared
                            && image.instance == args[0]
                            && image.key == key
                            && image.kind == kind
                    }) {
                        return Ok(handle);
                    }
                }
                let payload = if args[0] == 0 {
                    let super::resources::Key::Id(id) = key else {
                        self.set_error(mem, 1814)?;
                        return Ok(0);
                    };
                    if !(if kind == 1 {
                        matches!(id, 32512..=32518)
                    } else {
                        matches!(id, 32512..=32516 | 32640..=32646 | 32648..=32651 | 32671..=32672)
                    }) {
                        self.set_error(mem, 1814)?;
                        return Ok(0);
                    }
                    // Logical stock icon/cursor identity. Native drawing of these
                    // stock objects is separate from the window/frame presenter.
                    Vec::new()
                } else {
                    if self.modules.image(args[0]).is_none() {
                        self.set_error(mem, 6)?;
                        return Ok(0);
                    }
                    let group =
                        self.resource(mem, args[0], if kind == 1 { 14 } else { 12 }, &key)?;
                    let Some(group) = group else {
                        self.set_error(mem, 1814)?;
                        return Ok(0);
                    };
                    if kind != 1 {
                        self.set_error(mem, 50)?;
                        return Ok(0);
                    }
                    if group.len() < 6 || group[..4] != [0, 0, 1, 0] {
                        return Err("invalid icon group header".into());
                    }
                    let count = u16::from_le_bytes(group[4..6].try_into().unwrap()) as usize;
                    if count == 0 || count > 256 || 6 + count * 14 > group.len() {
                        return Err("truncated icon group".into());
                    }
                    let index = (0..count)
                        .min_by_key(|&i| {
                            let width = if group[6 + i * 14] == 0 {
                                256u16
                            } else {
                                group[6 + i * 14] as u16
                            };
                            width.abs_diff(32)
                        })
                        .unwrap();
                    let offset = 6 + index * 14;
                    let id =
                        u16::from_le_bytes(group[offset + 12..offset + 14].try_into().unwrap());
                    let length =
                        u32::from_le_bytes(group[offset + 8..offset + 12].try_into().unwrap());
                    let Some(payload) =
                        self.resource(mem, args[0], 3, &super::resources::Key::Id(id as u32))?
                    else {
                        self.set_error(mem, 1814)?;
                        return Ok(0);
                    };
                    if payload.len() != length as usize {
                        return Err("icon group payload size mismatch".into());
                    }
                    if payload.len() < 40 {
                        return Err("truncated icon DIB".into());
                    }
                    let word = |at| u32::from_le_bytes(payload[at..at + 4].try_into().unwrap());
                    if word(0) != 40
                        || word(16) != 0
                        || u16::from_le_bytes(payload[14..16].try_into().unwrap()) != 32
                    {
                        self.set_error(mem, 50)?;
                        return Ok(0);
                    }
                    let width = word(4);
                    let doubled_height = word(8);
                    if width == 0
                        || width > 256
                        || doubled_height == 0
                        || doubled_height > 512
                        || !doubled_height.is_multiple_of(2)
                        || u16::from_le_bytes(payload[12..14].try_into().unwrap()) != 1
                    {
                        return Err("invalid icon DIB dimensions/planes".into());
                    }
                    let height = doubled_height / 2;
                    let required = 40 + width * height * 4 + width.div_ceil(32) * 4 * height;
                    if payload.len() < required as usize {
                        return Err("truncated icon bitmap/mask".into());
                    }
                    // Retain the actual resource; no substitute icon makes a
                    // missing guest resource look successfully loaded.
                    payload
                };
                if self.user.images.len() >= 4096 || self.user.next_image >= super::TEB + 0x620000 {
                    self.set_error(mem, 8)?;
                    return Ok(0);
                }
                let handle = self.user.next_image;
                self.user.next_image += 16;
                self.user.images.insert(
                    handle,
                    Image {
                        instance: args[0],
                        key,
                        kind,
                        shared,
                        _payload: payload,
                    },
                );
                Ok(handle)
            }
            "DestroyIcon" | "DestroyCursor" => {
                let kind = if name == "DestroyIcon" { 1 } else { 2 };
                let Some(image) = self
                    .user
                    .images
                    .get(&args[0])
                    .filter(|image| image.kind == kind)
                else {
                    self.set_error(mem, 6)?;
                    return Ok(0);
                };
                if !image.shared {
                    self.user.images.remove(&args[0]);
                }
                Ok(1)
            }
            "SetCursor" => {
                if args[0] != 0
                    && !self
                        .user
                        .images
                        .get(&args[0])
                        .is_some_and(|image| image.kind == 2)
                {
                    self.set_error(mem, 6)?;
                    return Ok(0);
                }
                let previous = self.user.cursor;
                self.user.cursor = args[0];
                Ok(previous)
            }
            "GetCursor" => Ok(self.user.cursor),
            // The native input bridge delivers legacy mouse messages. Mouse
            // promotion to WM_POINTER is not installed in this guest profile.
            "IsMouseInPointerEnabled" => Ok(0),
            "EnableMouseInPointer" => {
                if args[0] != 0 {
                    self.set_error(mem, 50)?; // WM_POINTER promotion has no backend.
                    Ok(0)
                } else {
                    Ok(1)
                }
            }
            "ShowCursor" => {
                let count = self
                    .user
                    .cursor_counts
                    .entry(self.threads.active)
                    .or_default();
                *count = if args[0] != 0 {
                    count.saturating_add(1)
                } else {
                    count.saturating_sub(1)
                };
                Ok(*count as u32 as u64)
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
pub(super) fn gdi_arity(name: &str) -> Option<usize> {
    if matches!(name, "GetDC" | "ReleaseDC") {
        None
    } else {
        gdi::arity(name)
    }
}
