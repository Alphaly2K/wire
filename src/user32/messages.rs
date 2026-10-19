//! Typed posted-message, TIMERPROC and blocking queue coordination.
use super::*;
use crate::win32::catalog::MessageApi as M;

#[allow(clippy::too_many_arguments)]
pub(crate) fn call(
    id: M,
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
    match id {
        M::SetTimer | M::KillTimer => {
            let owner = if a[0] == 0 {
                tid
            } else if let Some(w) = state.user32.windows.get(&a[0]) {
                w.tid
            } else {
                state.last_error = 1400;
                return value(0, "invalid timer HWND".into());
            };
            if id == M::KillTimer {
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
        M::PostA | M::PostW | M::PostThreadA | M::PostThreadW | M::Quit => {
            let (owner, hwnd) = if matches!(id, M::PostThreadA | M::PostThreadW) {
                (a[0], 0)
            } else if id == M::Quit || a[0] == 0 {
                (tid, 0)
            } else if let Some(w) = state.user32.windows.get(&a[0]) {
                (w.tid, a[0])
            } else {
                state.last_error = 1400;
                return value(0, "invalid posted HWND".into());
            };
            let message = if id == M::Quit {
                Message {
                    hwnd: 0,
                    message: 0x12,
                    wparam: a[0],
                    lparam: 0,
                    time: clock_ms(state.start),
                    synthetic_paint: false,
                    hardware_input: false,
                }
            } else {
                Message {
                    hwnd,
                    message: a[1],
                    wparam: a[2],
                    lparam: a[3],
                    time: clock_ms(state.start),
                    synthetic_paint: false,
                    hardware_input: false,
                }
            };
            let ok = state
                .user32
                .put_message(&mut state.last_error, sched, owner, message);
            value(ok as u32, "queued message".into())
        }
        M::PeekA | M::PeekW | M::GetA | M::GetW | M::Wait => {
            sched.take_wake();
            state.user32.prepare_queue(state.start, tid);
            let wait_only = id == M::Wait;
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
                    if id.get() { u32::MAX } else { 0 },
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
                *state.user32.queue_changed.entry(tid).or_default() &= !message_bits(msg.message);
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
                if id.get() || a[4] & 1 != 0 {
                    queue.remove(index);
                    if msg.hardware_input {
                        state
                            .user32
                            .keyboard
                            .consumed(tid, msg.message, msg.wparam, msg.lparam);
                    }
                }
                let result = if id.get() && msg.message == 0x12 {
                    0
                } else {
                    1
                };
                return value(
                    result,
                    format!("message {:#x} HWND={:#x}", msg.message, msg.hwnd),
                );
            }
            if id.peek() {
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
        M::DispatchA | M::DispatchW => {
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
        M::Translate => {
            let mut words = [0; 5];
            for (i, n) in words.iter_mut().enumerate() {
                *n = mem
                    .read_u32(a[0] + i as u32 * 4)
                    .map_err(|e| e.to_string())?;
            }
            let msg = words[1];
            if matches!(msg, 0x100 | 0x101 | 0x104 | 0x105) {
                if matches!(msg, 0x100 | 0x104) {
                    let text = state.user32.keyboard.translate(words);
                    let wide = state
                        .user32
                        .windows
                        .get(&words[0])
                        .is_some_and(|w| state.user32.classes[&w.class].wide);
                    let chars: Vec<u32> = if wide {
                        text.into_iter().map(u32::from).collect()
                    } else {
                        crate::codepage::encode(&String::from_utf16_lossy(&text), 932)
                            .map_err(|e| format!("keyboard encoding: {e}"))?
                            .0
                            .into_iter()
                            .map(u32::from)
                            .collect()
                    };
                    for c in chars {
                        state.user32.put_message(
                            &mut state.last_error,
                            sched,
                            tid,
                            Message {
                                hwnd: words[0],
                                message: if msg == 0x104 { 0x106 } else { 0x102 },
                                wparam: c,
                                lparam: words[3],
                                time: clock_ms(state.start),
                                synthetic_paint: false,
                                hardware_input: false,
                            },
                        );
                    }
                }
                return value(1, "translated host text for a keyboard message".into());
            }
            value(0, String::new())
        }
        _ => unreachable!(),
    }
}
