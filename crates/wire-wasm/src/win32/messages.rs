//! Posted/native message ownership and checked MSG output for both guest ABIs.
use super::{
    call::{peek_slot, Call},
    catalog::{ApiId, Contract, MessageApi as M, WindowControlApi as W},
    user, ServiceError, Services,
};
use crate::{
    machine::{Mode, State, WindowInput},
    memory::{Memory, WRITE},
    process::{threads::PendingKind, Loaded},
};
use std::collections::{BTreeMap, VecDeque};
const LIMIT: usize = 4096;
pub(super) const MASK: u32 = 0x1dff;
fn bits(message: u64) -> u32 {
    match message {
        0x100..=0x109 => 1,
        0x200 => 2,
        0x201..=0x20e => 4,
        0x245..=0x24f => 0x1000,
        0x113 => 0x10,
        0xf => 0x20,
        0xff => 0x400,
        _ => 0x108,
    }
}
#[derive(Clone, Copy)]
struct Timer {
    deadline: Option<u64>,
    period: u64,
    procedure: u64,
}

#[derive(Clone, Copy)]
struct Message {
    words: [u64; 5],
    tid: u32,
    point: [i32; 2],
    hardware: bool,
    pointer: Option<user::mouse_pointer::Snapshot>,
    paint: bool,
    sequence: u64,
}
#[derive(Default)]
struct Metadata {
    extra: u64,
    time: u32,
    position: u32,
    seen: [u64; 13],
    changed: u32,
}
#[derive(Default)]
pub(crate) struct Queue {
    events: VecDeque<Message>,
    metadata: BTreeMap<u32, Metadata>,
    sequence: u64,
    timers: BTreeMap<(u32, u64, u64), Timer>,
    next_timer: u64,
    clock: u64,
}
impl Queue {
    pub(crate) fn remove_raw(&mut self, packets: &[(u64, user::raw_input::Packet)]) {
        self.events.retain(|m| {
            !m.hardware
                || m.words[1] != 0xff
                || packets.binary_search_by_key(&m.words[3], |p| p.0).is_err()
        });
    }
    pub fn remove_window(&mut self, window: u64) {
        self.events.retain(|m| m.words[0] != window);
        self.timers.retain(|(_, hwnd, _), _| *hwnd != window);
    }
    pub fn exit(&mut self, tid: u32) {
        self.events.retain(|m| m.tid != tid);
        self.metadata.remove(&tid);
        self.timers.retain(|(owner, _, _), _| *owner != tid);
    }
    fn index(&self, tid: u32, window: u64, min: u32, max: u32, mode: Mode) -> Option<usize> {
        self.events.iter().position(|m| {
            m.tid == tid
                && (m.words[1] == 0x12
                    || ((window == 0
                        || window == m.words[0]
                        || window == mode.address(u64::MAX) && m.words[0] == 0)
                        && (min == 0 && max == 0 || (min..=max).contains(&(m.words[1] as u32)))))
        })
    }
    fn push(&mut self, words: [u64; 5], tid: u32, point: [i32; 2], hardware: bool) {
        self.sequence += 1;
        self.metadata.entry(tid).or_default().changed |= bits(words[1]);
        self.events.push_back(Message {
            words,
            tid,
            point,
            hardware,
            pointer: None,
            paint: false,
            sequence: self.sequence,
        });
    }
    pub(crate) fn clear_paint(&mut self, window: u64) {
        self.events.retain(|m| !(m.paint && m.words[0] == window));
    }
    pub(crate) fn refresh_paint(
        &mut self,
        tid: u32,
        windows: &[u64],
        point: [i32; 2],
        sample: u64,
    ) {
        self.clock = self.clock.max(sample);
        let windows = windows
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        self.events
            .retain(|m| !m.paint || m.tid != tid || windows.contains(&m.words[0]));
        for window in windows {
            if self.capacity(1)
                && !self
                    .events
                    .iter()
                    .any(|m| m.tid == tid && m.words[0] == window && m.words[1] == 0xf)
            {
                self.push(
                    [window, 0xf, 0, 0, (self.clock / 10000) as u32 as u64],
                    tid,
                    point,
                    false,
                );
                self.events.back_mut().unwrap().paint = true;
            }
        }
    }
    pub(super) fn needs_time(&self, tid: u32) -> bool {
        self.timers
            .iter()
            .any(|((owner, _, _), t)| *owner == tid && t.deadline.is_some())
    }
    pub(super) fn checked(&mut self, tid: u32, mask: u32) {
        // A category query must leave unrelated input eligible for a later wait.
        let m = self.metadata.entry(tid).or_default();
        for n in 0..13 {
            if mask & (1 << n) != 0 {
                m.seen[n] = self.sequence;
            }
        }
        m.changed &= !mask;
    }
    pub(super) fn ready(&self, tid: u32, mask: u32, old: bool) -> bool {
        self.events.iter().any(|m| {
            m.tid == tid
                && (0..13).any(|n| {
                    bits(m.words[1]) & mask & (1 << n) != 0
                        && (old || m.sequence > self.metadata.get(&tid).map_or(0, |s| s.seen[n]))
                })
        })
    }
    pub(super) fn refresh(&mut self, tid: u32, sample: u64, point: [i32; 2]) {
        self.clock = self.clock.max(sample);
        let now = self.clock;
        let due = self
            .timers
            .iter()
            .filter_map(|(&key, t)| {
                (key.0 == tid && t.deadline.is_some_and(|d| d <= now)).then_some(key)
            })
            .collect::<Vec<_>>();
        for key in due {
            let pending = self.events.iter().any(|m| {
                m.tid == tid && m.words[0] == key.1 && m.words[1] == 0x113 && m.words[2] == key.2
            });
            if !pending && !self.capacity(1) {
                continue;
            }
            let timer = self.timers.get_mut(&key).unwrap();
            let deadline = timer.deadline.unwrap();
            timer.deadline = ((now - deadline) / timer.period)
                .checked_add(1)
                .and_then(|n| n.checked_mul(timer.period))
                .and_then(|step| deadline.checked_add(step));
            let procedure = timer.procedure;
            if !pending {
                self.push(
                    [key.1, 0x113, key.2, procedure, (now / 10000) as u32 as u64],
                    tid,
                    point,
                    false,
                );
            }
        }
    }
    fn capacity(&self, count: usize) -> bool {
        self.events
            .len()
            .checked_add(count)
            .is_some_and(|n| n <= LIMIT)
            && self.sequence.checked_add(count as u64).is_some()
    }
}
pub(super) fn implemented(id: ApiId) -> bool {
    matches!(
        id,
        ApiId::Message(
            M::SetTimer
                | M::KillTimer
                | M::QueueStatus
                | M::PostA
                | M::PostW
                | M::PostThreadA
                | M::PostThreadW
                | M::Quit
                | M::PeekA
                | M::PeekW
                | M::GetA
                | M::GetW
                | M::DispatchA
                | M::DispatchW
                | M::Translate
                | M::Wait
                | M::Extra
                | M::SetExtra
                | M::Time
                | M::Position
        )
    )
}
pub(super) fn uses_clock(
    id: ApiId,
    state: &State,
    memory: &Memory,
    image: Option<&Loaded>,
) -> bool {
    match id {
        ApiId::Message(
            M::SetTimer | M::PostA | M::PostW | M::PostThreadA | M::PostThreadW | M::Quit,
        ) => true,
        ApiId::Message(M::GetA | M::GetW | M::PeekA | M::PeekW | M::Wait) => {
            image.is_some_and(|i| i.user.message_clock(i.threads.active, MASK))
        }
        ApiId::Message(M::QueueStatus) => image.is_some_and(|i| {
            peek_slot(state, memory, 0)
                .is_some_and(|m| i.user.message_clock(i.threads.active, m as u32))
        }),
        ApiId::Message(M::DispatchA | M::DispatchW) => {
            let Some(pointer) = peek_slot(state, memory, 0) else {
                return false;
            };
            let mut b = [0; 48];
            let x86 = memory.mode() == Mode::X86;
            if memory
                .observe(pointer, &mut b[..if x86 { 28 } else { 48 }])
                .is_err()
            {
                return false;
            }
            let msg = if x86 { 4 } else { 8 };
            let lp = if x86 { 12 } else { 24 };
            u32::from_le_bytes(b[msg..msg + 4].try_into().unwrap()) == 0x113
                && b[lp..lp + if x86 { 4 } else { 8 }].iter().any(|b| *b != 0)
        }
        ApiId::Message(M::Translate) => {
            let Some(pointer) = peek_slot(state, memory, 0) else {
                return false;
            };
            let offset = if memory.mode() == Mode::X86 { 4 } else { 8 };
            let mut b = [0; 4];
            memory
                .observe(memory.mode().address(pointer.wrapping_add(offset)), &mut b)
                .is_ok()
                && matches!(u32::from_le_bytes(b), 0x100 | 0x104)
        }
        _ => false,
    }
}
pub(crate) fn read(memory: &Memory, pointer: u64) -> Result<[u64; 5], crate::memory::Fault> {
    let mode = memory.mode();
    let mut b = vec![0; if mode == Mode::X86 { 28 } else { 48 }];
    memory.read(pointer, &mut b)?;
    Ok(if mode == Mode::X86 {
        std::array::from_fn(|n| {
            u64::from(u32::from_le_bytes(b[n * 4..n * 4 + 4].try_into().unwrap()))
        })
    } else {
        [
            u64::from_le_bytes(b[..8].try_into().unwrap()),
            u64::from(u32::from_le_bytes(b[8..12].try_into().unwrap())),
            u64::from_le_bytes(b[16..24].try_into().unwrap()),
            u64::from_le_bytes(b[24..32].try_into().unwrap()),
            u64::from(u32::from_le_bytes(b[32..36].try_into().unwrap())),
        ]
    })
}
fn encode(mode: Mode, m: Message) -> Vec<u8> {
    let mut b = vec![0; if mode == Mode::X86 { 28 } else { 48 }];
    if mode == Mode::X86 {
        for (n, v) in m.words.into_iter().enumerate() {
            b[n * 4..n * 4 + 4].copy_from_slice(&(v as u32).to_le_bytes());
        }
    } else {
        b[..8].copy_from_slice(&m.words[0].to_le_bytes());
        b[8..12].copy_from_slice(&(m.words[1] as u32).to_le_bytes());
        b[16..24].copy_from_slice(&m.words[2].to_le_bytes());
        b[24..32].copy_from_slice(&m.words[3].to_le_bytes());
        b[32..36].copy_from_slice(&(m.words[4] as u32).to_le_bytes());
    }
    let at = if mode == Mode::X86 { 20 } else { 36 };
    for (n, v) in m.point.into_iter().enumerate() {
        b[at + n * 4..at + n * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    b
}
fn blocked(image: &mut Loaded, call: &Call, pc: u64) -> Result<u64, ServiceError> {
    if image.pending().is_none_or(|p| p.pc != pc) {
        image.set_pending(call.clone(), PendingKind::Message, pc);
    }
    Err(ServiceError::Blocked)
}
fn value(
    id: M,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: &mut Loaded,
    sample: u64,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    let tid = image.threads.active;
    let mode = memory.mode();
    let time = (sample / 10000) as u32;
    match id {
        M::SetTimer | M::KillTimer => {
            if a[0] != 0 {
                let Some(w) = image.user.windows.get(&a[0]) else {
                    return user::error(memory, state, 1400);
                };
                if w.tid != u64::from(tid) {
                    return user::error(memory, state, 5);
                }
            }
            let q = &mut image.user.messages;
            if id == M::KillTimer {
                return Ok(u64::from(q.timers.remove(&(tid, a[0], a[1])).is_some()));
            }
            q.clock = q.clock.max(sample);
            let period = u64::from((a[2] as u32).clamp(10, 0x7fffffff)) * 10000;
            let Some(deadline) = q.clock.checked_add(period) else {
                return user::error(memory, state, 87);
            };
            let existing = q.timers.contains_key(&(tid, a[0], a[1]));
            let generated = a[1] == 0 || a[0] == 0 && !existing;
            let timer_id = if generated {
                let mut timer_id = q.next_timer.max(1);
                while q.timers.contains_key(&(tid, a[0], timer_id))
                    && timer_id < mode.address(u64::MAX)
                {
                    timer_id += 1;
                }
                if timer_id >= mode.address(u64::MAX) {
                    return user::error(memory, state, 8);
                }
                timer_id
            } else {
                a[1]
            };
            if !q.timers.contains_key(&(tid, a[0], timer_id)) && q.timers.len() >= 1024 {
                return user::error(memory, state, 8);
            }
            q.timers.insert(
                (tid, a[0], timer_id),
                Timer {
                    deadline: Some(deadline),
                    period,
                    procedure: a[3],
                },
            );
            if generated {
                q.next_timer = timer_id + 1;
            }
            Ok(timer_id)
        }
        M::QueueStatus => {
            let mask = a[0] as u32;
            if mask & !MASK != 0 {
                return user::error(memory, state, 87);
            }
            image.user.refresh_messages(tid, sample, mask);
            let q = &mut image.user.messages;
            let current = q
                .events
                .iter()
                .filter(|m| m.tid == tid)
                .fold(0, |s, m| s | bits(m.words[1]))
                & mask;
            let changed = q.metadata.get(&tid).map_or(0, |m| m.changed) & mask;
            q.checked(tid, mask);
            Ok(u64::from((current << 16) | changed))
        }
        M::Extra | M::Time | M::Position => {
            let m = image.user.messages.metadata.entry(tid).or_default();
            Ok(match id {
                M::Extra => m.extra,
                M::Time => u64::from(m.time),
                _ => u64::from(m.position),
            })
        }
        M::SetExtra => {
            let m = image.user.messages.metadata.entry(tid).or_default();
            Ok(std::mem::replace(&mut m.extra, a[0]))
        }
        M::Quit | M::PostA | M::PostW | M::PostThreadA | M::PostThreadW => {
            let thread = matches!(id, M::PostThreadA | M::PostThreadW);
            let quit = id == M::Quit;
            let window = if thread || quit { 0 } else { a[0] };
            let owner = if thread {
                if !image
                    .threads
                    .get(a[0] as u32)
                    .is_some_and(|t| t.exit.is_none())
                {
                    return user::error(memory, state, 1444);
                }
                a[0] as u32
            } else if window == 0 {
                tid
            } else {
                let Some(w) = image.user.windows.get(&window) else {
                    return user::error(memory, state, if window == 65535 { 50 } else { 1400 });
                };
                w.tid as u32
            };
            let message = if quit { 0x12 } else { a[1] as u32 };
            if !quit && matches!(message, 1 | 0xc | 0xd | 0x24 | 0x46 | 0x47 | 0x4a | 0x83) {
                return user::error(memory, state, 1159);
            }
            if !image.user.messages.capacity(1) {
                return user::error(memory, state, 1816);
            }
            image.user.messages.push(
                [
                    window,
                    u64::from(message),
                    if quit { a[0] } else { a[2] },
                    if quit { 0 } else { a[3] },
                    u64::from(time),
                ],
                owner,
                image.user.cursor.position(),
                false,
            );
            image.threads.changed();
            Ok(u64::from(!quit))
        }
        M::PeekA | M::PeekW | M::GetA | M::GetW => {
            image.user.refresh_messages(tid, sample, MASK);
            let get = id.get();
            let invalid_window = a[1] != 0
                && a[1] != mode.address(u64::MAX)
                && !image.user.windows.contains_key(&a[1]);
            if invalid_window || (a[2] != 0 || a[3] != 0) && a[2] > a[3] {
                user::error(memory, state, if invalid_window { 1400 } else { 87 })?;
                return Ok(if get { u64::from(u32::MAX) } else { 0 });
            }
            if !get && a[4] & !3 != 0 {
                return user::error(memory, state, 50);
            }
            let index = image
                .user
                .messages
                .index(tid, a[1], a[2] as u32, a[3] as u32, mode);
            let Some(index) = index else {
                if get {
                    memory.check(a[0], if mode == Mode::X86 { 28 } else { 48 }, WRITE)?;
                }
                image.user.messages.checked(tid, MASK);
                return if get {
                    blocked(image, call, state.ip)
                } else {
                    Ok(0)
                };
            };
            let m = image.user.messages.events[index];
            memory.write(a[0], &encode(mode, m))?;
            let metadata = image.user.messages.metadata.entry(tid).or_default();
            metadata.extra = 0;
            metadata.time = m.words[4] as u32;
            metadata.position = u32::from(m.point[0] as u16) | (u32::from(m.point[1] as u16) << 16);
            metadata.seen = [image.user.messages.sequence; 13];
            metadata.changed = 0;
            if let Some(pointer) = m.pointer {
                image
                    .user
                    .mouse_pointer
                    .current
                    .insert(u64::from(tid), pointer);
            }
            if get || a[4] & 1 != 0 {
                image.user.messages.events.remove(index);
                if m.hardware {
                    if m.words[1] == 0xff {
                        if let Some(packet) = image.user.raw_input.packets.get_mut(&m.words[3]) {
                            packet.queued = false;
                            if packet.no_legacy {
                                let [message, key, lp] = packet.input;
                                image.user.keyboard.consumed64(packet.tid, message, key, lp);
                            }
                        }
                    }
                    let input = m.pointer.map_or(
                        [m.words[1] as u32, m.words[2] as u32, m.words[3] as u32],
                        |p| p.input,
                    );
                    image
                        .user
                        .keyboard
                        .consumed64(u64::from(tid), input[0], input[1], input[2]);
                }
            }
            Ok(u64::from(!get || m.words[1] != 0x12))
        }
        M::Wait => {
            image.user.refresh_messages(tid, sample, MASK);
            if image.user.messages.ready(tid, MASK, false) {
                image.user.messages.checked(tid, MASK);
                Ok(1)
            } else {
                blocked(image, call, state.ip)
            }
        }
        M::Translate => {
            let words = read(memory, a[0])?;
            if !matches!(words[1], 0x100 | 0x101 | 0x104 | 0x105) {
                return Ok(0);
            }
            if matches!(words[1], 0x100 | 0x104) {
                if !image.user.messages.capacity(64) {
                    return user::error(memory, state, 1816);
                }
                let text = image.user.keyboard.translate64(words);
                let wide = image.user.windows.get(&words[0]).is_none_or(|w| w.wide);
                let chars = if wide {
                    text.into_iter().map(u64::from).collect::<Vec<_>>()
                } else {
                    crate::codepage::encode(&String::from_utf16_lossy(&text), 932)
                        .map_err(|_| ServiceError::Unavailable("invalid translated text encoding"))?
                        .0
                        .into_iter()
                        .map(u64::from)
                        .collect()
                };
                for c in chars {
                    image.user.messages.push(
                        [
                            words[0],
                            if words[1] == 0x104 { 0x106 } else { 0x102 },
                            c,
                            words[3],
                            u64::from(time),
                        ],
                        tid,
                        image.user.cursor.position(),
                        false,
                    );
                }
                image.threads.changed();
            }
            Ok(1)
        }
        _ => unreachable!(),
    }
}
pub(super) fn dispatch(
    contract: &Contract,
    services: &Services,
    state: &mut State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
    sample: u64,
) -> Result<(), ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "message services require a loaded image",
    ))?;
    let mut call = if let Some(p) = image.pending().filter(|p| p.pc == state.ip) {
        p.call.clone()
    } else {
        Call::capture(memory.mode(), state, memory, contract)?
    };
    let ApiId::Message(id) = contract.id else {
        unreachable!()
    };
    if id.dispatch() {
        let words = read(memory, call.arguments[0])?;
        if words[1] == 0x113 && words[3] != 0 {
            call.arguments[..5].copy_from_slice(&[
                words[3],
                words[0],
                0x113,
                words[2],
                (sample / 10000) as u32 as u64,
            ]);
            image.timer_call(call, state, memory, services)?;
        } else if words[0] == 0 {
            call.complete(state, 0);
        } else {
            call.arguments[..4].copy_from_slice(&words[..4]);
            image.window_call(
                if id == M::DispatchW {
                    W::SendW
                } else {
                    W::SendA
                },
                call,
                state,
                memory,
                services,
            )?;
        }
    } else {
        let answer = value(id, &call, state, memory, image, sample)?;
        image.clear_pending();
        call.complete(state, answer);
    }
    state.work = state.work.wrapping_add(1);
    Ok(())
}

impl Loaded {
    pub(crate) fn queue_window_events(
        &mut self,
        mode: Mode,
        inputs: &[WindowInput],
    ) -> Result<(), String> {
        if inputs.len() > 256 {
            return Err("host event batch exceeds capacity".into());
        }
        let raw_count = inputs
            .iter()
            .filter(|i| {
                let message = i.event.message;
                let usage = if (0x200..=0x20e).contains(&message) {
                    2
                } else if matches!(message, 0x100 | 0x101 | 0x104 | 0x105) {
                    6
                } else {
                    return false;
                };
                self.user.windows.contains_key(&i.event.window)
                    && self.user.raw_input.registrations.contains_key(&usage)
            })
            .count();
        if !self.user.messages.capacity(inputs.len() + raw_count) {
            return Err("USER message queue capacity exceeded".into());
        }
        self.user.raw_input.check_capacity(
            raw_count,
            if mode == Mode::X86 { 1 << 32 } else { u64::MAX },
        )?;
        for input in inputs {
            if !matches!(input.event.message,6..=8|0x10|0x1c|0x100..=0x109|0x200..=0x20e|0x8000..=0xffff)
            {
                return Err("unsupported host window message".into());
            }
            if let Some(w) = self.user.windows.get(&input.event.window) {
                self.user
                    .client_origin(input.event.window)
                    .map_err(|_| "invalid host window coordinates")?;
                if let Some(capture) = self
                    .user
                    .input
                    .get(&(w.tid as u32))
                    .map(|i| i.capture)
                    .filter(|h| self.user.windows.contains_key(h))
                {
                    self.user
                        .client_origin(capture)
                        .map_err(|_| "invalid capture coordinates")?;
                }
            }
        }
        for input in inputs {
            let e = input.event;
            let Some(w) = self.user.windows.get(&e.window) else {
                continue;
            };
            if matches!(e.message, 0x102 | 0x103 | 0x106 | 0x107 | 0x109)
                && self
                    .user
                    .raw_input
                    .registrations
                    .get(&6)
                    .is_some_and(|r| r.flags & 0x30 == 0x30)
            {
                continue;
            }
            let tid = w.tid as u32;
            let origin = self
                .user
                .client_origin(e.window)
                .expect("preflighted window coordinates");
            let point = if let Some(p) = input.point {
                p
            } else if (0x200..=0x20e).contains(&e.message) {
                let p = [
                    e.lparam as u16 as i16 as i32,
                    (e.lparam >> 16) as u16 as i16 as i32,
                ];
                if matches!(e.message, 0x20a | 0x20e) {
                    p
                } else {
                    [origin[0].wrapping_add(p[0]), origin[1].wrapping_add(p[1])]
                }
            } else {
                self.user.cursor.position()
            };
            let point = self
                .user
                .clip
                .map_or(point, |clip| user::cursor::clamp(point, clip));
            let mut window = e.window;
            let mut lparam = mode.address(e.lparam);
            if self.user.clip.is_some() && (0x200..=0x20e).contains(&e.message) {
                let p = if matches!(e.message, 0x20a | 0x20e) {
                    point
                } else {
                    [
                        point[0].wrapping_sub(origin[0]),
                        point[1].wrapping_sub(origin[1]),
                    ]
                };
                lparam = u64::from(p[0] as u16) | u64::from(p[1] as u16) << 16;
            }

            if let Some(input) = self.user.input.get(&tid) {
                if (0x200..=0x20e).contains(&e.message) && input.capture != 0 {
                    if self.user.windows.contains_key(&input.capture) {
                        let origin = self
                            .user
                            .client_origin(input.capture)
                            .expect("preflighted capture coordinates");
                        window = input.capture;
                        if !matches!(e.message, 0x20a | 0x20e) {
                            lparam = u64::from(point[0].wrapping_sub(origin[0]) as u16)
                                | (u64::from(point[1].wrapping_sub(origin[1]) as u16) << 16);
                        }
                    }
                } else if (0x100..=0x109).contains(&e.message) && input.focus != 0 {
                    window = input.focus;
                }
            }
            if matches!(e.message, 6..=8 | 0x1c) {
                let input = self.user.input.entry(tid).or_default();
                match e.message {
                    7 => input.focus = e.window,
                    8 => {
                        if input.focus == e.window {
                            input.focus = 0;
                        }
                    }
                    6 => {
                        if e.wparam & 65535 != 0 {
                            input.active = e.window;
                            self.user.foreground = e.window;
                        } else {
                            if input.active == e.window {
                                input.active = 0;
                            }
                            if self.user.foreground == e.window {
                                self.user.foreground = 0;
                            }
                        }
                    }
                    _ => {
                        if e.wparam != 0 {
                            self.user.foreground = e.window;
                        } else if self.user.foreground == e.window {
                            self.user.foreground = 0;
                        }
                    }
                }
            }
            let mut words = [
                window,
                u64::from(e.message),
                mode.address(e.wparam),
                lparam,
                u64::from(input.time),
            ];
            self.user.cursor.set_position(point);
            if self.user.keyboard.host64(words) {
                let mut no_legacy = false;
                if let Some((handle, suppress)) = self.user.raw_input.host(
                    window,
                    [e.message, e.wparam as u32, lparam as u32],
                    point,
                    [
                        super::display::profile::WIDTH,
                        super::display::profile::HEIGHT,
                    ],
                    |target| self.user.windows.get(&target).map(|w| w.tid),
                )? {
                    no_legacy = suppress;
                    let packet = &self.user.raw_input.packets[&handle];
                    self.user.messages.push(
                        [packet.window, 0xff, 0, handle, u64::from(input.time)],
                        packet.tid as u32,
                        point,
                        true,
                    );
                }
                let pointer = self.user.mouse_pointer.promote(
                    window,
                    [e.message, e.wparam as u32, lparam as u32],
                    point,
                    input.time,
                );
                if let Some((message, wp, snapshot)) = pointer {
                    words[1] = u64::from(message);
                    words[2] = wp;
                    words[3] = mode.address(
                        (u32::from(point[0] as u16) | u32::from(point[1] as u16) << 16) as i32
                            as i64 as u64,
                    );
                    self.user.messages.push(words, tid, point, true);
                    self.user.messages.events.back_mut().unwrap().pointer = Some(snapshot);
                } else if !no_legacy {
                    self.user.messages.push(words, tid, point, true);
                }
            }
        }
        self.threads.changed();
        Ok(())
    }
}
