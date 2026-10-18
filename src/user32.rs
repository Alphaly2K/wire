//! Process-owned Win32 window classes. Presentation is a separate platform layer.
use crate::api::{ApiLogger, ApiState, ApiTable};
use crate::cpu::{Cpu, GuestCallOutcome};
use crate::mem::GuestMem;
use crate::thread::{Sched, ThreadState, WaitTarget, Wake};
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

mod classes;
mod control;
mod messages;
mod objects;
mod queries;
mod window_state;
pub(crate) use classes::{call as class_call, register_system_class};
pub use classes::{find_class, WindowClass};
pub(crate) use control::call as control_call;
pub(crate) use messages::call as message_call;
pub(crate) use queries::desktop as desktop_query;
pub(crate) use window_state::call as window_state_call;

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
    let Some(w) = state.user32.windows.get(&hwnd) else {
        return;
    };
    let tid = w.tid;
    let time = clock_ms(state);
    if !state
        .user32
        .keyboard
        .host([hwnd, message, wparam, lparam, time])
    {
        return;
    }
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
            hardware_input: true,
        },
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
    None
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

fn remove_window(state: &mut ApiState, sched: &mut Sched, hwnd: u32) {
    state.user32.keyboard.remove_window(hwnd);
    let pending: Vec<_> = state
        .user32
        .sent
        .iter()
        .filter(|(_, m)| m.args[0] == hwnd && !m.processing && m.result.is_none())
        .map(|(&id, m)| (id, m.sender))
        .collect();
    for (id, sender) in pending {
        state.user32.sent.get_mut(&id).unwrap().result = Some(0);
        wake_sender(sched, id, sender);
    }
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

fn wake_sender(sched: &mut Sched, id: u32, sender: u32) {
    if let Some(i)=sched.threads.iter().position(|t|t.tid==sender && matches!(t.state,ThreadState::Waiting {target:WaitTarget::SentMessage(n),..} if n==id)) {
        sched.wake_thread(i,Wake::Retest);
    }
}
pub(crate) fn thread_exit(state: &mut ApiState, sched: &mut Sched, tid: u32) {
    let windows: Vec<_> = state
        .user32
        .windows
        .iter()
        .filter_map(|(&h, w)| (w.tid == tid).then_some(h))
        .collect();
    for h in windows {
        remove_window(state, sched, h);
    }
    let pending: Vec<_> = state
        .user32
        .sent
        .iter()
        .filter(|(_, m)| m.receiver == tid && m.result.is_none())
        .map(|(&id, m)| (id, m.sender))
        .collect();
    for (id, sender) in pending {
        state.user32.sent.get_mut(&id).unwrap().result = Some(0);
        wake_sender(sched, id, sender);
    }
    state
        .user32
        .sent
        .retain(|_, m| m.sender != tid || m.processing);
    state.user32.queues.remove(&tid);
    state.user32.queue_changed.remove(&tid);
    state.user32.cursor.exit(tid);
    state.user32.keyboard.exit(tid);
    state.user32.timers.retain(|&(owner, _, _), _| owner != tid);
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
                    hardware_input: false,
                });
                *state.user32.queue_changed.entry(tid).or_default() |= 0x10;
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
    let bits = message_bits(message.message);
    *state.user32.queue_changed.entry(tid).or_default() |= bits;
    if matches!(
        sched.threads[i].state,
        ThreadState::Waiting {
            target: WaitTarget::Messages(_),
            ..
        }
    ) || matches!(&sched.threads[i].state, ThreadState::Waiting {
        target: WaitTarget::MessageObjects(_,_,mask),.. } if *mask & bits != 0)
    {
        sched.wake_thread(i, Wake::Retest);
    }
    true
}
fn message_bits(message: u32) -> u32 {
    match message {
        0x100..=0x109 => 1,
        0x200 => 2,
        0x201..=0x20e => 4,
        0x113 => 0x10,
        0xf => 0x20,
        0xff => 0x400,
        _ => 0x108,
    }
}
pub(crate) fn message_wait_ready(state: &mut ApiState, tid: u32, mask: u32, old: bool) -> bool {
    refresh_timers(state, tid);
    let time = clock_ms(state);
    let q = state.user32.queues.entry(tid).or_default();
    q.retain(|m| {
        !m.synthetic_paint
            || state
                .user32
                .windows
                .get(&m.hwnd)
                .is_some_and(|w| w.update.is_some() && w.style & 0x10000000 != 0)
    });
    for (&hwnd, w) in &state.user32.windows {
        if w.tid == tid
            && w.style & 0x10000000 != 0
            && w.update.is_some()
            && !q.iter().any(|m| m.hwnd == hwnd && m.message == 0xf)
        {
            q.push_back(Message {
                hwnd,
                message: 0xf,
                wparam: 0,
                lparam: 0,
                time,
                synthetic_paint: true,
                hardware_input: false,
            });
            *state.user32.queue_changed.entry(tid).or_default() |= 0x20;
        }
    }
    if old {
        q.iter().any(|m| message_bits(m.message) & mask != 0)
    } else {
        state.user32.queue_changed.get(&tid).copied().unwrap_or(0) & mask != 0
    }
}
