/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; see COPYING.LIB. Window/control flow previously
 * in user32_common.rs is adapted from Wine 10.0.
 */
//! Typed process coordination for lifecycle and synchronous WNDPROC calls.
use super::*;
use crate::win32::catalog::WindowControlApi as W;
type ResultValue = Result<(GuestCallOutcome, String), String>;
#[allow(clippy::too_many_arguments)]
pub(crate) fn call(
    id: W,
    a: &crate::api::Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    if matches!(id, W::CreateA | W::CreateW) {
        classes::ensure_builtin(mem, api, state, a[1], id.wide())?;
    }
    match id {
        W::CallA | W::CallW => {
            let outcome = invoke(cpu, mem, api, state, log, sched, a[0], &a[1..5], max_instr)?;
            Ok((outcome, "guest WndProc forwarding".into()))
        }
        W::Enable | W::Parent | W::Move | W::Position => {
            crate::x86::process::window_geometry::call(
                id, a, cpu, mem, api, state, log, sched, max_instr,
            )
        }
        W::Update => crate::x86::process::window_geometry::update(
            a, cpu, mem, api, state, log, sched, max_instr,
        ),
        _ => window_dispatch(id, a, cpu, mem, api, state, log, sched, max_instr),
    }
}
#[allow(clippy::too_many_arguments)]
fn window_dispatch(
    id: W,
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
    if id.default_procedure() {
        if a[1] == 0x10 {
            return window_dispatch(W::Destroy, a, cpu, mem, api, state, log, sched, max_instr);
        }
        if let Some(ret) = crate::user32_common::default_text(id.wide(), a, mem, &mut state.user32)?
        {
            return value(ret, "default window text".into());
        }
        if a[1] == 0x47 {
            return crate::x86::process::window_geometry::default_windowpos_changed(
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
            let (result, _) = crate::x86::process::painting::call(
                crate::win32::catalog::PaintingApi::Begin,
                &pa,
                cpu,
                mem,
                api,
                state,
                log,
                sched,
                max_instr,
            )?;
            if !matches!(result, GuestCallOutcome::Returned(_)) {
                return Ok((result, "default paint transferred".into()));
            }
            crate::x86::process::painting::call(
                crate::win32::catalog::PaintingApi::End,
                &pa,
                cpu,
                mem,
                api,
                state,
                log,
                sched,
                max_instr,
            )?;
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
    if matches!(id, W::CreateA | W::CreateW) {
        let wide = id.wide();
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
            props: Default::default(),
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
                fields[9] = classes::persistent_strings(mem, state, &title)?[class.wide as usize];
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
                        state.user32.remove_window(
                            &mut state.graphics,
                            &mut state.menus,
                            sched,
                            hwnd,
                        );
                        if !matches!(result, GuestCallOutcome::Returned(_)) {
                            return Ok((result, "creation cleanup transferred".into()));
                        }
                    }
                    return value(0, "WndProc rejected creation".into());
                }
                Ok(transfer) => {
                    state
                        .user32
                        .remove_window(&mut state.graphics, &mut state.menus, sched, hwnd);
                    return Ok((transfer, "creation transferred".into()));
                }
                Err(error) => {
                    state
                        .user32
                        .remove_window(&mut state.graphics, &mut state.menus, sched, hwnd);
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
    if matches!(id, W::SendA | W::SendW) {
        let sender = sched.current_tid();
        if let Some((&id, msg)) = state
            .user32
            .sent
            .iter()
            .find(|(_, m)| m.sender == sender && m.result.is_some() && m.args == a[..4])
        {
            let result = msg.result.unwrap();
            state.user32.sent.remove(&id);
            sched.take_wake();
            return value(
                result,
                "cross-thread WndProc completed or owner was removed".into(),
            );
        }
    }
    let Some(window) = state.user32.windows.get_mut(&a[0]) else {
        state.last_error = 1400;
        return value(0, "invalid HWND".into());
    };
    match id {
        W::SendA | W::SendW => {
            if window.tid != sched.current_tid() {
                let owner = window.tid;
                let sender = sched.current_tid();
                if let Some((&id, request)) =
                    state.user32.sent.iter().find(|(_, m)| m.sender == sender)
                {
                    if let Some(result) = request.result {
                        state.user32.sent.remove(&id);
                        sched.take_wake();
                        return value(result, "cross-thread WndProc completed on its owner".into());
                    }
                    sched.block_current(cpu, WaitTarget::SentMessage(id), None)?;
                    return Ok((
                        GuestCallOutcome::Transferred,
                        "waiting for WndProc owner".into(),
                    ));
                }
                if state.user32.sent.len() >= 256 {
                    state.last_error = 8;
                    return value(0, "sent message limit".into());
                }
                let Some(receiver) = sched
                    .threads
                    .iter()
                    .position(|t| t.tid == owner && !matches!(t.state, ThreadState::Exited(_)))
                else {
                    state.last_error = 1400;
                    return value(0, "WndProc owner exited".into());
                };
                let id = state.user32.next_sent;
                state.user32.next_sent = state
                    .user32
                    .next_sent
                    .checked_add(1)
                    .ok_or("sent message ID exhausted")?;
                state.user32.sent.insert(
                    id,
                    SentMessage {
                        sender,
                        receiver: owner,
                        args: a[..4].try_into().unwrap(),
                        processing: false,
                        result: None,
                    },
                );
                if matches!(
                    sched.threads[receiver].state,
                    ThreadState::Waiting {
                        target: WaitTarget::Messages(_) | WaitTarget::SentMessage(_),
                        ..
                    }
                ) || matches!(sched.threads[receiver].state,ThreadState::Waiting {
                        target:WaitTarget::MessageObjects(_,_,mask),..} if mask&0x40!=0)
                {
                    sched.wake_thread(receiver, Wake::Retest);
                }
                sched.block_current(cpu, WaitTarget::SentMessage(id), None)?;
                return Ok((
                    GuestCallOutcome::Transferred,
                    "queued synchronous WndProc for owner thread".into(),
                ));
            }
            let proc = window.wndproc;
            let result = invoke(cpu, mem, api, state, log, sched, proc, &a[..4], max_instr)?;
            Ok((result, "synchronous guest WndProc".into()))
        }
        W::Show => {
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
        W::Destroy => {
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
            state
                .user32
                .remove_window(&mut state.graphics, &mut state.menus, sched, a[0]);
            value(1, "window destroyed".into())
        }
        _ => unreachable!(),
    }
}
