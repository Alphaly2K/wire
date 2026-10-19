//! Process-owned Win32 window classes. Presentation is a separate platform layer.
use crate::api::{ApiLogger, ApiState, ApiTable};
use crate::cpu::{Cpu, GuestCallOutcome};
use crate::mem::GuestMem;
use crate::thread::{Sched, ThreadState, WaitTarget, Wake};
use crate::x86::process::callbacks::invoke;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

mod classes;
mod control;
mod dialog;
mod lifecycle;
mod messages;
mod objects;
mod paint;
mod queries;
mod queues;
mod window_state;
pub(crate) use classes::{call as class_call, register_system_class};
pub use classes::{find_class, WindowClass};
pub(crate) use control::call as control_call;
pub(crate) use dialog::call as dialog_call;
pub(crate) use messages::call as message_call;
pub(crate) use paint::call as paint_call;
pub(crate) use queries::desktop as desktop_query;
use queues::{clock_ms, message_bits};
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn service_sent(
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<Option<GuestCallOutcome>, String> {
    let tid = sched.current_tid();
    loop {
        let Some((&id, msg)) = state
            .user32
            .sent
            .iter()
            .find(|(_, m)| m.receiver == tid && !m.processing && m.result.is_none())
        else {
            return Ok(None);
        };
        let args = msg.args;
        state.user32.sent.get_mut(&id).unwrap().processing = true;
        let result = if let Some(w) = state.user32.windows.get(&args[0]) {
            let proc = w.wndproc;
            invoke(cpu, mem, api, state, log, sched, proc, &args, max_instr)?
        } else {
            GuestCallOutcome::Returned(0)
        };
        let GuestCallOutcome::Returned(value) = result else {
            return Ok(Some(result));
        };
        if sched.threads.iter().any(|t| {
            t.tid == state.user32.sent[&id].sender && matches!(t.state, ThreadState::Exited(_))
        }) {
            state.user32.sent.remove(&id);
            continue;
        }
        let msg = state
            .user32
            .sent
            .get_mut(&id)
            .ok_or("sent message removed during callback")?;
        msg.result = Some(value);
        let sender = msg.sender;
        if let Some(i) = sched.threads.iter().position(|t| {
            t.tid == sender
                && matches!(t.state,
            ThreadState::Waiting {target:WaitTarget::SentMessage(n),..} if n==id)
        }) {
            sched.wake_thread(i, Wake::Retest);
        }
    }
}

/// Queue a host input event at a guest scheduling boundary.
pub fn host_message(
    state: &mut ApiState,
    sched: &mut Sched,
    hwnd: u32,
    message: u32,
    wparam: u32,
    lparam: u32,
) {
    state.user32.host_message(
        &mut state.last_error,
        sched,
        state.start,
        [hwnd, message, wparam, lparam],
    );
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn enumerate_monitors(
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<(GuestCallOutcome, String), String> {
    if a[0] != 0 || a[2] == 0 {
        state.last_error = if a[0] != 0 { 50 } else { 87 };
        return Ok((
            GuestCallOutcome::Returned(0),
            "unsupported DC or invalid monitor callback".into(),
        ));
    }
    let mut rect = [0i32, 0, 1024, 768];
    if a[1] != 0 {
        let mut bytes = [0; 16];
        mem.read_bytes(a[1], &mut bytes)
            .map_err(|e| e.to_string())?;
        let clip: [i32; 4] = std::array::from_fn(|i| {
            i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
        });
        rect = [
            rect[0].max(clip[0]),
            rect[1].max(clip[1]),
            rect[2].min(clip[2]),
            rect[3].min(clip[3]),
        ];
    }
    if rect[0] >= rect[2] || rect[1] >= rect[3] {
        return Ok((
            GuestCallOutcome::Returned(1),
            "no monitor intersects the clipping rectangle".into(),
        ));
    }
    let p = state
        .heap
        .alloc(mem, state.process_heap, 16, false)
        .map_err(|e| e.to_string())?;
    mem.write_bytes(
        p,
        &rect
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .map_err(|e| e.to_string())?;
    let result = invoke(
        cpu,
        mem,
        api,
        state,
        log,
        sched,
        a[2],
        &[0x40000001, 0, p, a[3]],
        max_instr,
    );
    state.heap.free(state.process_heap, p);
    result.map(|value| (value, "enumerated the logical 1024x768 monitor".into()))
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
    if name == "RedrawWindow" {
        return Some(paint::redraw(
            a, cpu, mem, api, state, log, sched, max_instr,
        ));
    }
    // Compatibility facade for callers with names. Bound API gateways use
    // the typed ID directly and never resolve the export on the hot path.
    use crate::win32::catalog::{resolve, ApiId, Architecture, Resolution};
    if let Resolution::Bound(contract) = resolve(Architecture::X86, dll, name) {
        return match contract.id {
            ApiId::Dialog(id) if !id.message_box() => Some(dialog_call(
                id, a, cpu, mem, api, state, log, sched, max_instr,
            )),
            ApiId::Painting(id) => Some(paint_call(
                id, a, cpu, mem, api, state, log, sched, max_instr,
            )),
            _ => None,
        };
    }
    None
}
