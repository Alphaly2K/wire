//! Process-owned Win32 window classes. Presentation is a separate platform layer.
use crate::api::{ApiLogger, ApiState, ApiTable};
use crate::cpu::{Cpu, GuestCallOutcome};
use crate::mem::GuestMem;
use crate::thread::{Sched, ThreadState, WaitTarget, Wake};
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct WindowClass {
    pub name: String,
    pub fields: [u32; 11], // style through hIconSm, excluding cbSize
    wide: bool,
    name_ptrs: [u32; 2],
    menu_ptrs: [u32; 2],
}

pub struct User32 {
    pub classes: BTreeMap<u16, WindowClass>,
    pub windows: BTreeMap<u32, Window>,
    pub timers: BTreeMap<(u32, u32, u32), Timer>, // tid, HWND, id
    queues: BTreeMap<u32, VecDeque<Message>>,
    next_atom: u32,
    next_window: u32,
    next_timer: u32,
    pub(crate) focus: u32,
    pub(crate) active: u32,
    pub(crate) foreground: u32,
    pub(crate) capture: u32,
    pub(crate) cursor_pos: [i32; 2],
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
    synthetic_paint: bool,
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
    destroying: bool,
}
impl Window {
    pub(crate) fn frame(&self) -> [i32; 4] {
        crate::user32_geometry::frame(
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
            next_atom: 0xc000,
            next_window: 0x4001_0000,
            next_timer: 1,
            focus: 0,
            active: 0,
            foreground: 0,
            capture: 0,
            cursor_pos: [0; 2],
        }
    }
}

pub(crate) fn host_message(
    state: &mut ApiState,
    sched: &mut Sched,
    hwnd: u32,
    message: u32,
    wparam: u32,
    lparam: u32,
) {
    let Some(w) = state.user32.windows.get(&hwnd) else {
        return;
    };
    let tid = w.tid;
    let time = clock_ms(state);
    put_message(
        state,
        sched,
        tid,
        Message {
            hwnd,
            message,
            wparam,
            lparam,
            time,
            synthetic_paint: false,
        },
    );
}

fn persistent_strings(
    mem: &mut GuestMem,
    state: &mut ApiState,
    text: &str,
) -> Result<[u32; 2], String> {
    let ansi = crate::codepage::encode(text, 932)
        .map_err(|e| format!("class name encoding: {e}"))?
        .0;
    let wide: Vec<u16> = text.encode_utf16().collect();
    let a = state.scratch_alloc(mem, ansi.len() as u32 + 1, 1);
    let w = state.scratch_alloc(mem, (wide.len() as u32 + 1) * 2, 2);
    mem.write_bytes(a, &ansi).map_err(|e| e.to_string())?;
    mem.write_u8(a + ansi.len() as u32, 0)
        .map_err(|e| e.to_string())?;
    for (i, value) in wide.into_iter().chain([0]).enumerate() {
        mem.write_u16(w + i as u32 * 2, value)
            .map_err(|e| e.to_string())?;
    }
    Ok([a, w])
}

pub fn find_class(
    mem: &GuestMem,
    state: &ApiState,
    id: u32,
    instance: u32,
    wide: bool,
) -> Result<Option<u16>, String> {
    let name = if id > 0xffff {
        Some(
            crate::file_api::read_name(mem, id, wide)
                .map_err(|e| format!("class name: {e}"))?
                .to_lowercase(),
        )
    } else {
        None
    };
    Ok(state.user32.classes.iter().find_map(|(&atom, class)| {
        let matches_name = name
            .as_ref()
            .map_or(atom as u32 == id, |n| class.name.to_lowercase() == *n);
        (matches_name && (class.fields[4] == instance || class.fields[0] & 0x4000 != 0))
            .then_some(atom)
    }))
}

#[allow(clippy::too_many_arguments)]
pub fn dispatch(
    dll: &str,
    name: &str,
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Option<Result<(GuestCallOutcome, String), String>> {
    if dll != "user32.dll" {
        return None;
    }
    if let Some(result) = crate::user32_geometry::adjust(name, a, mem, state) {
        return Some(result.map(|n| {
            (
                GuestCallOutcome::Returned(n),
                "96 DPI window frame geometry".into(),
            )
        }));
    }
    if let Some(result) =
        crate::user32_paint::dispatch(name, a, cpu, mem, api, state, log, sched, max_instr)
    {
        return Some(result);
    }
    if let Some(result) =
        crate::user32_common::dispatch(name, a, cpu, mem, api, state, log, sched, max_instr)
    {
        return Some(result);
    }
    if matches!(name, "EnumWindows" | "EnumThreadWindows") {
        return Some(crate::user32_enum::enumerate(
            name, a, cpu, mem, api, state, log, sched, max_instr,
        ));
    }
    if matches!(
        name,
        "RegisterClassA"
            | "RegisterClassW"
            | "RegisterClassExA"
            | "RegisterClassExW"
            | "GetClassInfoA"
            | "GetClassInfoW"
            | "GetClassInfoExA"
            | "GetClassInfoExW"
            | "UnregisterClassA"
            | "UnregisterClassW"
    ) {
        return Some(
            class_dispatch(name, a, mem, state)
                .map(|(value, note)| (GuestCallOutcome::Returned(value), note)),
        );
    }
    if matches!(
        name,
        "CreateWindowExA"
            | "CreateWindowExW"
            | "DestroyWindow"
            | "DefWindowProcA"
            | "DefWindowProcW"
            | "SendMessageA"
            | "SendMessageW"
            | "GetWindowLongA"
            | "GetWindowLongW"
            | "SetWindowLongA"
            | "SetWindowLongW"
            | "IsWindow"
            | "IsWindowVisible"
            | "IsWindowUnicode"
            | "GetClientRect"
            | "GetWindowRect"
            | "GetParent"
            | "GetWindowThreadProcessId"
            | "ShowWindow"
    ) {
        return Some(window_dispatch(
            name, a, cpu, mem, api, state, log, sched, max_instr,
        ));
    }
    if matches!(
        name,
        "SetTimer"
            | "KillTimer"
            | "PostMessageA"
            | "PostMessageW"
            | "PostThreadMessageA"
            | "PostThreadMessageW"
            | "PostQuitMessage"
            | "PeekMessageA"
            | "PeekMessageW"
            | "GetMessageA"
            | "GetMessageW"
            | "DispatchMessageA"
            | "DispatchMessageW"
            | "TranslateMessage"
            | "WaitMessage"
            | "CallWindowProcA"
            | "CallWindowProcW"
    ) {
        return Some(message_dispatch(
            name, a, cpu, mem, api, state, log, sched, max_instr,
        ));
    }
    None
}

fn class_dispatch(
    name: &str,
    a: &crate::api::Args,
    mem: &mut GuestMem,
    state: &mut ApiState,
) -> Result<(u32, String), String> {
    let wide = name.ends_with('W');
    let extended = name.contains("Ex");
    let fail = |state: &mut ApiState, code| {
        state.last_error = code;
        Ok((0, format!("window class error {code}")))
    };
    if name.starts_with("Register") {
        if extended && mem.read_u32(a[0]).map_err(|e| e.to_string())? != 48 {
            return fail(state, 87);
        }
        let mut fields = [0; 11];
        for (i, value) in fields
            .iter_mut()
            .take(if extended { 11 } else { 10 })
            .enumerate()
        {
            *value = mem
                .read_u32(a[0] + (i as u32 + extended as u32) * 4)
                .map_err(|e| e.to_string())?;
        }
        if fields[1] == 0 || fields[9] <= 0xffff || fields[2] > 4096 || fields[3] > 4096 {
            return fail(state, 87);
        }
        let text = crate::file_api::read_name(mem, fields[9], wide)
            .map_err(|e| format!("class name: {e}"))?;
        if text.is_empty() || text.encode_utf16().count() > 255 {
            return fail(state, 87);
        }
        if find_class(mem, state, fields[9], fields[4], wide)?.is_some() {
            return fail(state, 1410);
        }
        if state.user32.next_atom > 0xffff {
            return fail(state, 8);
        }
        let name_ptrs = persistent_strings(mem, state, &text)?;
        let menu_ptrs = if fields[8] <= 0xffff {
            [fields[8]; 2]
        } else {
            let menu = crate::file_api::read_name(mem, fields[8], wide)
                .map_err(|e| format!("class menu: {e}"))?;
            persistent_strings(mem, state, &menu)?
        };
        let atom = state.user32.next_atom as u16;
        state.user32.next_atom += 1;
        state.user32.classes.insert(
            atom,
            WindowClass {
                name: text.clone(),
                fields,
                wide,
                name_ptrs,
                menu_ptrs,
            },
        );
        return Ok((atom as u32, format!("registered {text}")));
    }
    let unregister = name.starts_with("Unregister");
    let (instance, id) = if unregister {
        (a[1], a[0])
    } else {
        (a[0], a[1])
    };
    let Some(atom) = find_class(mem, state, id, instance, wide)? else {
        return fail(state, 1411);
    };
    if unregister {
        if state.user32.windows.values().any(|w| w.class == atom) {
            return fail(state, 1412);
        }
        state.user32.classes.remove(&atom);
        return Ok((1, "class unregistered".into()));
    }
    let class = state.user32.classes.get(&atom).unwrap();
    let mut fields = class.fields;
    fields[8] = class.menu_ptrs[wide as usize];
    fields[9] = class.name_ptrs[wide as usize];
    let mut bytes = Vec::new();
    if extended {
        bytes.extend(48u32.to_le_bytes());
    }
    for value in fields.into_iter().take(if extended { 11 } else { 10 }) {
        bytes.extend(value.to_le_bytes());
    }
    mem.write_bytes(a[2], &bytes).map_err(|e| e.to_string())?;
    Ok((1, format!("class {} atom={atom:#x}", class.name)))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn invoke(
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    proc: u32,
    a: &[u32],
    max_instr: u64,
) -> Result<GuestCallOutcome, String> {
    let saved = cpu.save_ctx();
    let result = cpu.call_guest(mem, api, state, log, sched, proc, a, max_instr);
    if matches!(result, Ok(GuestCallOutcome::Returned(_)) | Err(_)) {
        cpu.load_ctx(&saved);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn window_dispatch(
    name: &str,
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<(GuestCallOutcome, String), String> {
    let value = |value, note: String| Ok((GuestCallOutcome::Returned(value), note));
    if name.starts_with("DefWindowProc") {
        if a[1] == 0x10 {
            return window_dispatch(
                "DestroyWindow",
                a,
                cpu,
                mem,
                api,
                state,
                log,
                sched,
                max_instr,
            );
        }
        if let Some(ret) = crate::user32_common::default_text(name, a, mem, state)? {
            return value(ret, "default window text".into());
        }
        if a[1] == 0x47 {
            return crate::user32_common::default_windowpos_changed(
                a, cpu, mem, api, state, log, sched, max_instr,
            );
        }
        if a[1] == 0x83 {
            if let Some(w) = state.user32.windows.get(&a[0]) {
                let f = w.frame();
                let mut bytes = [0; 16];
                mem.read_bytes(a[3], &mut bytes)
                    .map_err(|e| e.to_string())?;
                let mut r: [i32; 4] = std::array::from_fn(|i| {
                    i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
                for i in 0..4 {
                    r[i] = if i < 2 {
                        r[i].saturating_add(f[i])
                    } else {
                        r[i].saturating_sub(f[i])
                    };
                }
                r[2] = r[2].max(r[0]);
                r[3] = r[3].max(r[1]);
                mem.write_bytes(
                    a[3],
                    &r.into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>(),
                )
                .map_err(|e| e.to_string())?;
            }
            return value(0, "default nonclient geometry".into());
        }
        if a[1] == 0xf {
            let mut pa = [0; 16];
            pa[0] = a[0];
            pa[1] = state.scratch_alloc(mem, 64, 4);
            let (result, _) = crate::user32_paint::dispatch(
                "BeginPaint",
                &pa,
                cpu,
                mem,
                api,
                state,
                log,
                sched,
                max_instr,
            )
            .unwrap()?;
            if !matches!(result, GuestCallOutcome::Returned(_)) {
                return Ok((result, "default paint transferred".into()));
            }
            crate::user32_paint::dispatch(
                "EndPaint", &pa, cpu, mem, api, state, log, sched, max_instr,
            )
            .unwrap()?;
            return value(0, "default paint validated".into());
        }
        if a[1] == 0x14 {
            let brush = state
                .user32
                .windows
                .get(&a[0])
                .and_then(|w| state.user32.classes.get(&w.class))
                .map_or(0, |c| c.fields[7]);
            if (1..=31).contains(&brush) {
                let color = match brush - 1 {
                    5 => 0xffffff,
                    8 | 18 => 0,
                    15 => 0xf0f0f0,
                    _ => 0xc0c0c0,
                };
                let result = state.graphics.fill_color(mem, a[2], color).is_ok() as u32;
                return value(result, "default system-color background".into());
            }
            return value(0, "no supported class background brush".into());
        }
        // Default acceptance of WM_NCCREATE; the implemented default subset
        // has no application action for WM_CREATE/DESTROY or unknown messages.
        return value((a[1] == 0x81) as u32, "default WndProc subset".into());
    }
    if name.starts_with("CreateWindowEx") {
        let wide = name.ends_with('W');
        let Some(atom) = find_class(mem, state, a[1], a[10], wide)? else {
            state.last_error = 1407;
            return value(0, "window class not registered".into());
        };
        if a[8] != 0 && a[8] != 0xffff_fffd && !state.user32.windows.contains_key(&a[8]) {
            state.last_error = 1400;
            return value(0, "invalid parent window".into());
        }
        let class = state.user32.classes.get(&atom).unwrap().clone();
        let hwnd = state.user32.next_window;
        state.user32.next_window = hwnd.checked_add(4).ok_or("window handle exhaustion")?;
        let rect = [a[4], a[5], a[6], a[7]].map(|n| if n == 0x8000_0000 { 0 } else { n as i32 });
        let window = Window {
            class: atom,
            wndproc: class.fields[1],
            instance: a[10],
            parent: a[8],
            menu: a[9],
            style: a[3],
            ex_style: a[0],
            rect,
            user_data: 0,
            extra: vec![0; class.fields[3] as usize],
            wide: class.wide,
            tid: sched.current_tid(),
            text: crate::menu::read_text(mem, a[2], wide)
                .map_err(|e| format!("window title error {e}"))?
                .unwrap_or_default(),
            destroying: false,
            update: None,
            erase: false,
        };
        let create = state.scratch_alloc(mem, 48, 4);
        let mut fields = [
            a[11], a[10], a[9], a[8], a[7], a[6], a[5], a[4], a[3], a[2], a[1], a[0],
        ];
        if wide != class.wide {
            fields[10] = if a[1] <= 0xffff {
                a[1]
            } else {
                class.name_ptrs[class.wide as usize]
            };
            if a[2] != 0 {
                let title = crate::file_api::read_name(mem, a[2], wide)
                    .map_err(|e| format!("window title: {e}"))?;
                fields[9] = persistent_strings(mem, state, &title)?[class.wide as usize];
            }
        }
        for (i, field) in fields.into_iter().enumerate() {
            mem.write_u32(create + i as u32 * 4, field)
                .map_err(|e| e.to_string())?;
        }
        state.user32.windows.insert(hwnd, window);
        for message in [0x81, 1] {
            // WM_NCCREATE precedes WM_CREATE
            let proc = match state.user32.windows.get(&hwnd) {
                Some(window) => window.wndproc,
                None => return value(0, "window destroyed during creation".into()),
            };
            let result = invoke(
                cpu,
                mem,
                api,
                state,
                log,
                sched,
                proc,
                &[hwnd, message, 0, create],
                max_instr,
            );
            match result {
                Ok(GuestCallOutcome::Returned(result))
                    if (message == 0x81 && result != 0) || (message == 1 && result != u32::MAX) => {
                }
                Ok(GuestCallOutcome::Returned(_)) => {
                    let proc = state.user32.windows.get(&hwnd).map(|w| w.wndproc);
                    if let Some(proc) = proc {
                        let mut result = GuestCallOutcome::Returned(0);
                        for notification in if message == 1 {
                            &[2, 0x82][..]
                        } else {
                            &[0x82][..]
                        } {
                            result = invoke(
                                cpu,
                                mem,
                                api,
                                state,
                                log,
                                sched,
                                proc,
                                &[hwnd, *notification, 0, 0],
                                max_instr,
                            )?;
                            if !matches!(result, GuestCallOutcome::Returned(_)) {
                                break;
                            }
                        }
                        remove_window(state, hwnd);
                        if !matches!(result, GuestCallOutcome::Returned(_)) {
                            return Ok((result, "creation cleanup transferred".into()));
                        }
                    }
                    return value(0, "WndProc rejected creation".into());
                }
                Ok(transfer) => {
                    remove_window(state, hwnd);
                    return Ok((transfer, "creation transferred".into()));
                }
                Err(error) => {
                    remove_window(state, hwnd);
                    return Err(error);
                }
            }
        }
        return value(
            if state.user32.windows.contains_key(&hwnd) {
                hwnd
            } else {
                0
            },
            format!("created {} HWND={hwnd:#x}", class.name),
        );
    }
    if matches!(name, "IsWindow" | "IsWindowVisible" | "IsWindowUnicode") {
        let result = state.user32.windows.get(&a[0]).is_some_and(|w| match name {
            "IsWindowVisible" => w.style & 0x1000_0000 != 0,
            "IsWindowUnicode" => w.wide,
            _ => true,
        });
        return value(result as u32, String::new());
    }
    let parent_origin = if name == "GetWindowRect" {
        if let Some(w) = state
            .user32
            .windows
            .get(&a[0])
            .filter(|w| w.style & 0x40000000 != 0)
        {
            crate::user32_geometry::client_origin(state, w.parent)?
        } else {
            [0; 2]
        }
    } else {
        [0; 2]
    };
    let Some(window) = state.user32.windows.get_mut(&a[0]) else {
        state.last_error = 1400;
        return value(0, "invalid HWND".into());
    };
    match name {
        "GetWindowLongA" | "GetWindowLongW" | "SetWindowLongA" | "SetWindowLongW" => {
            let setting = name.starts_with("Set");
            if setting && a[1] == (-4i32) as u32 && a[2] == 0 {
                state.last_error = 87;
                return value(0, "null WndProc".into());
            }
            let slot = match a[1] as i32 {
                -4 => Some(&mut window.wndproc),
                -6 => Some(&mut window.instance),
                -8 => Some(&mut window.parent),
                -12 => Some(&mut window.menu),
                -16 => Some(&mut window.style),
                -20 => Some(&mut window.ex_style),
                -21 => Some(&mut window.user_data),
                _ => None,
            };
            let old = if let Some(slot) = slot {
                let old = *slot;
                if setting {
                    *slot = a[2];
                }
                old
            } else if (a[1] as i32) >= 0
                && (a[1] as usize)
                    .checked_add(4)
                    .is_some_and(|end| end <= window.extra.len())
            {
                let bytes = &mut window.extra[a[1] as usize..a[1] as usize + 4];
                let old = u32::from_le_bytes(bytes.try_into().unwrap());
                if setting {
                    bytes.copy_from_slice(&a[2].to_le_bytes());
                }
                old
            } else {
                state.last_error = 1413;
                return value(0, "invalid window long index".into());
            };
            value(old, String::new())
        }
        "GetParent" => value(window.parent, String::new()),
        "GetWindowThreadProcessId" => {
            if a[1] != 0 {
                mem.write_u32(a[1], 4242).map_err(|e| e.to_string())?;
            }
            value(window.tid, String::new())
        }
        "GetClientRect" | "GetWindowRect" => {
            let [x, y, w, h] = window.rect;
            let rect = if name == "GetClientRect" {
                let size = window.client_size();
                [0, 0, size[0], size[1]]
            } else {
                let x = x.wrapping_add(parent_origin[0]);
                let y = y.wrapping_add(parent_origin[1]);
                [x, y, x.saturating_add(w), y.saturating_add(h)]
            };
            for (i, n) in rect.into_iter().enumerate() {
                mem.write_u32(a[1] + i as u32 * 4, n as u32)
                    .map_err(|e| e.to_string())?;
            }
            value(1, "logical window geometry".into())
        }
        "SendMessageA" | "SendMessageW" => {
            if window.tid != sched.current_tid() {
                return Err("cross-thread synchronous WndProc is unsupported".into());
            }
            let proc = window.wndproc;
            let result = invoke(cpu, mem, api, state, log, sched, proc, &a[..4], max_instr)?;
            Ok((result, "synchronous guest WndProc".into()))
        }
        "ShowWindow" => {
            let old = (window.style & 0x1000_0000 != 0) as u32;
            if a[1] == 0 {
                window.style &= !0x1000_0000;
            } else {
                window.style |= 0x1000_0000;
                let [width, height] = window.client_size();
                window.update = Some([0, 0, width, height]);
                window.erase = true;
            }
            let changed = old != (a[1] != 0) as u32;
            if changed {
                let proc = window.wndproc;
                let result = invoke(
                    cpu,
                    mem,
                    api,
                    state,
                    log,
                    sched,
                    proc,
                    &[a[0], 0x18, (a[1] != 0) as u32, 0],
                    max_instr,
                )?;
                if !matches!(result, GuestCallOutcome::Returned(_)) {
                    return Ok((result, "WM_SHOWWINDOW transferred".into()));
                }
            }
            value(old, "window visibility and paint state".into())
        }
        "DestroyWindow" => {
            if window.tid != sched.current_tid() {
                state.last_error = 5;
                return value(0, "window belongs to another thread".into());
            }
            if window.destroying {
                return value(0, "window already being destroyed".into());
            }
            window.destroying = true;
            for message in [2, 0x82] {
                let Some(proc) = state.user32.windows.get(&a[0]).map(|w| w.wndproc) else {
                    break;
                };
                let result = invoke(
                    cpu,
                    mem,
                    api,
                    state,
                    log,
                    sched,
                    proc,
                    &[a[0], message, 0, 0],
                    max_instr,
                )?;
                if !matches!(result, GuestCallOutcome::Returned(_)) {
                    return Ok((result, "destroy transferred".into()));
                }
            }
            remove_window(state, a[0]);
            value(1, "window destroyed".into())
        }
        _ => unreachable!(),
    }
}

fn remove_window(state: &mut ApiState, hwnd: u32) {
    state.graphics.remove_window(hwnd);
    for owner in [
        &mut state.user32.focus,
        &mut state.user32.active,
        &mut state.user32.foreground,
        &mut state.user32.capture,
    ] {
        if *owner == hwnd {
            *owner = 0;
        }
    }
    if let Some(window) = state.user32.windows.remove(&hwnd) {
        if window.style & 0x40000000 == 0 && window.menu != 0 {
            state.menus.destroy(window.menu);
        }
    }
    state
        .user32
        .timers
        .retain(|&(_, owner, _), _| owner != hwnd);
    for queue in state.user32.queues.values_mut() {
        queue.retain(|msg| msg.hwnd != hwnd);
    }
}

fn clock_ms(state: &ApiState) -> u32 {
    state.start.elapsed().as_millis() as u32
}

fn refresh_timers(state: &mut ApiState, tid: u32) {
    let now = Instant::now();
    let time = clock_ms(state);
    let queue = state.user32.queues.entry(tid).or_default();
    for (&(owner, hwnd, id), timer) in &mut state.user32.timers {
        if owner == tid && timer.deadline <= now {
            timer.deadline = now + timer.period;
            if !queue
                .iter()
                .any(|m| m.message == 0x113 && m.hwnd == hwnd && m.wparam == id)
            {
                queue.push_back(Message {
                    hwnd,
                    message: 0x113,
                    wparam: id,
                    lparam: timer.proc,
                    time,
                    synthetic_paint: false,
                });
            }
        }
    }
}

fn put_message(state: &mut ApiState, sched: &mut Sched, tid: u32, message: Message) -> bool {
    let Some(i) = sched
        .threads
        .iter()
        .position(|t| t.tid == tid && !matches!(t.state, ThreadState::Exited(_)))
    else {
        state.last_error = 1444;
        return false;
    };
    let queue = state.user32.queues.entry(tid).or_default();
    if queue.len() >= 10000 {
        state.last_error = 1816;
        return false;
    }
    queue.push_back(message);
    if matches!(
        sched.threads[i].state,
        ThreadState::Waiting {
            target: WaitTarget::Messages(_),
            ..
        }
    ) {
        sched.wake_thread(i, Wake::Retest);
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn message_dispatch(
    name: &str,
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<(GuestCallOutcome, String), String> {
    let value = |value, note: String| Ok((GuestCallOutcome::Returned(value), note));
    let tid = sched.current_tid();
    match name {
        "SetTimer" | "KillTimer" => {
            let owner = if a[0] == 0 {
                tid
            } else if let Some(w) = state.user32.windows.get(&a[0]) {
                w.tid
            } else {
                state.last_error = 1400;
                return value(0, "invalid timer HWND".into());
            };
            if name == "KillTimer" {
                return value(
                    state.user32.timers.remove(&(owner, a[0], a[1])).is_some() as u32,
                    String::new(),
                );
            }
            let id = if a[1] != 0
                && (a[0] != 0 || state.user32.timers.contains_key(&(owner, 0, a[1])))
            {
                a[1]
            } else {
                while state
                    .user32
                    .timers
                    .contains_key(&(owner, a[0], state.user32.next_timer))
                {
                    state.user32.next_timer = state
                        .user32
                        .next_timer
                        .checked_add(1)
                        .ok_or("timer id exhaustion")?;
                }
                let id = state.user32.next_timer;
                state.user32.next_timer = id.checked_add(1).ok_or("timer id exhaustion")?;
                id
            };
            let period = Duration::from_millis(a[2].clamp(10, 0x7fff_ffff) as u64);
            state.user32.timers.insert(
                (owner, a[0], id),
                Timer {
                    deadline: Instant::now() + period,
                    period,
                    proc: a[3],
                },
            );
            value(id, "registered message timer".into())
        }
        "PostMessageA" | "PostMessageW" | "PostThreadMessageA" | "PostThreadMessageW"
        | "PostQuitMessage" => {
            let (owner, hwnd) = if name.starts_with("PostThread") {
                (a[0], 0)
            } else if name == "PostQuitMessage" || a[0] == 0 {
                (tid, 0)
            } else if let Some(w) = state.user32.windows.get(&a[0]) {
                (w.tid, a[0])
            } else {
                state.last_error = 1400;
                return value(0, "invalid posted HWND".into());
            };
            let message = if name == "PostQuitMessage" {
                Message {
                    hwnd: 0,
                    message: 0x12,
                    wparam: a[0],
                    lparam: 0,
                    time: clock_ms(state),
                    synthetic_paint: false,
                }
            } else {
                Message {
                    hwnd,
                    message: a[1],
                    wparam: a[2],
                    lparam: a[3],
                    time: clock_ms(state),
                    synthetic_paint: false,
                }
            };
            let ok = put_message(state, sched, owner, message);
            value(ok as u32, "queued message".into())
        }
        "PeekMessageA" | "PeekMessageW" | "GetMessageA" | "GetMessageW" | "WaitMessage" => {
            sched.take_wake();
            refresh_timers(state, tid);
            let time = clock_ms(state);
            let queue = state.user32.queues.entry(tid).or_default();
            queue.retain(|m| {
                !m.synthetic_paint
                    || state
                        .user32
                        .windows
                        .get(&m.hwnd)
                        .is_some_and(|w| w.update.is_some() && w.style & 0x10000000 != 0)
            });
            // Paint follows already posted messages and timers; retrieving it
            // never validates the region. BeginPaint/ValidateRect does that.
            for (&hwnd, w) in &state.user32.windows {
                if w.tid == tid
                    && w.style & 0x10000000 != 0
                    && w.update.is_some()
                    && !queue.iter().any(|m| m.hwnd == hwnd && m.message == 0xf)
                {
                    queue.push_back(Message {
                        hwnd,
                        message: 0xf,
                        wparam: 0,
                        lparam: 0,
                        time,
                        synthetic_paint: true,
                    });
                }
            }
            let wait_only = name == "WaitMessage";
            if !wait_only
                && ((a[1] != 0 && a[1] != u32::MAX && !state.user32.windows.contains_key(&a[1]))
                    || ((a[2] != 0 || a[3] != 0) && a[2] > a[3]))
            {
                state.last_error = if a[1] != 0 && !state.user32.windows.contains_key(&a[1]) {
                    1400
                } else {
                    87
                };
                return value(
                    if name.starts_with("Get") { u32::MAX } else { 0 },
                    "invalid message filter".into(),
                );
            }
            let queue = state.user32.queues.entry(tid).or_default();
            let index = queue.iter().position(|msg| {
                wait_only
                    || msg.message == 0x12
                    || ((a[1] == 0 || a[1] == msg.hwnd || (a[1] == u32::MAX && msg.hwnd == 0))
                        && ((a[2] == 0 && a[3] == 0)
                            || (a[2] <= msg.message && msg.message <= a[3])))
            });
            if let Some(index) = index {
                if wait_only {
                    return value(1, String::new());
                }
                let msg = queue[index];
                let words = [
                    msg.hwnd,
                    msg.message,
                    msg.wparam,
                    msg.lparam,
                    msg.time,
                    0,
                    0,
                ];
                let bytes: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
                mem.write_bytes(a[0], &bytes).map_err(|e| e.to_string())?;
                if name.starts_with("Get") || a[4] & 1 != 0 {
                    queue.remove(index);
                }
                let result = if name.starts_with("Get") && msg.message == 0x12 {
                    0
                } else {
                    1
                };
                return value(
                    result,
                    format!("message {:#x} HWND={:#x}", msg.message, msg.hwnd),
                );
            }
            if name.starts_with("Peek") {
                return value(0, String::new());
            }
            let mut deadline = state
                .user32
                .timers
                .iter()
                .filter(|((owner, _, _), _)| *owner == tid)
                .map(|(_, timer)| timer.deadline)
                .min();
            if state.presenter.active() {
                let ui = Instant::now() + Duration::from_millis(16);
                deadline = Some(deadline.map_or(ui, |d| d.min(ui)));
            }
            sched.block_current(cpu, WaitTarget::Messages(tid), deadline)?;
            Ok((
                GuestCallOutcome::Transferred,
                "waiting for a message".into(),
            )) // redispatch this same stub when woken
        }
        "DispatchMessageA" | "DispatchMessageW" => {
            let words: Result<Vec<_>, _> = (0..5).map(|i| mem.read_u32(a[0] + i * 4)).collect();
            let words = words.map_err(|e| e.to_string())?;
            if state
                .user32
                .windows
                .get(&words[0])
                .is_some_and(|w| w.tid != tid)
            {
                return Err("cross-thread DispatchMessage is unsupported".into());
            }
            if words[1] == 0x113 && words[3] != 0 {
                let result = invoke(
                    cpu,
                    mem,
                    api,
                    state,
                    log,
                    sched,
                    words[3],
                    &[words[0], 0x113, words[2], words[4]],
                    max_instr,
                )?;
                return Ok((result, "guest TIMERPROC".into()));
            }
            let Some(proc) = state.user32.windows.get(&words[0]).map(|w| w.wndproc) else {
                return value(0, "thread message".into());
            };
            let result = invoke(
                cpu,
                mem,
                api,
                state,
                log,
                sched,
                proc,
                &words[..4],
                max_instr,
            )?;
            Ok((result, "queued guest WndProc".into()))
        }
        "CallWindowProcA" | "CallWindowProcW" => {
            let result = invoke(cpu, mem, api, state, log, sched, a[0], &a[1..5], max_instr)?;
            Ok((result, "guest WndProc forwarding".into()))
        }
        "TranslateMessage" => {
            let msg = mem.read_u32(a[0] + 4).map_err(|e| e.to_string())?;
            if matches!(msg, 0x100 | 0x101 | 0x104 | 0x105) {
                return Err(
                    "keyboard TranslateMessage requires the pending input/keymap backend".into(),
                );
            }
            value(0, String::new())
        }
        _ => unreachable!(),
    }
}
