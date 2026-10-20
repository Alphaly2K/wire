/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; see COPYING.LIB. Window/control flow previously
 * in user32_common.rs is adapted from Wine 10.0.
 */
//! Process executes lifecycle callbacks and synchronous window-message waits.
use super::{callbacks::invoke, GuestCallOutcome};
use crate::{
    api::{ApiLogger, ApiState, ApiTable, Args},
    cpu::Cpu,
    mem::GuestMem,
    thread::{Sched, WaitTarget},
    user32::{ControlRequest, CreationRequest, DefaultRequest},
    win32::catalog::WindowControlApi as W,
};
type ResultValue = Result<(GuestCallOutcome, String), String>;
#[allow(clippy::too_many_arguments)]
pub(crate) fn call(
    id: W,
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    match id {
        W::CreateA | W::CreateW => create(id, a, cpu, mem, api, state, log, sched, max_instr),
        W::CallA | W::CallW => {
            let outcome = invoke(cpu, mem, api, state, log, sched, a[0], &a[1..5], max_instr)?;
            Ok((outcome, "guest WndProc forwarding".into()))
        }
        W::Enable | W::Parent | W::Move | W::Position => {
            super::window_geometry::call(id, a, cpu, mem, api, state, log, sched, max_instr)
        }
        W::Update => super::window_geometry::update(a, cpu, mem, api, state, log, sched, max_instr),
        id if id.default_procedure() => default(id, a, cpu, mem, api, state, log, sched, max_instr),
        _ => control(id, a, cpu, mem, api, state, log, sched, max_instr),
    }
}
#[allow(clippy::too_many_arguments)]
fn create(
    id: W,
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    let value = |value, note: String| Ok((GuestCallOutcome::Returned(value), note));
    let (hwnd, create, name) =
        match state.window_creation_request(id, a, mem, api, sched.current_tid())? {
            CreationRequest::Value(n, note) => return value(n, note),
            CreationRequest::Window { hwnd, create, name } => (hwnd, create, name),
        };
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
                if (message == 0x81 && result != 0) || (message == 1 && result != u32::MAX) => {}
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
                    state
                        .user32
                        .remove_window(&mut state.graphics, &mut state.menus, sched, hwnd);
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
    value(
        if state.user32.windows.contains_key(&hwnd) {
            hwnd
        } else {
            0
        },
        format!("created {} HWND={hwnd:#x}", name),
    )
}
#[allow(clippy::too_many_arguments)]
fn control(
    id: W,
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    let value = |value, note: String| Ok((GuestCallOutcome::Returned(value), note));
    match state
        .user32
        .prepare_control(id, a, sched, &mut state.last_error)?
    {
        ControlRequest::Value(n, note) => value(n, note),
        ControlRequest::Send { procedure } => {
            let result = invoke(
                cpu,
                mem,
                api,
                state,
                log,
                sched,
                procedure,
                &a[..4],
                max_instr,
            )?;
            Ok((result, "synchronous guest WndProc".into()))
        }
        ControlRequest::Wait { message, note } => {
            sched.block_current(cpu, WaitTarget::SentMessage(message), None)?;
            Ok((GuestCallOutcome::Transferred, note.into()))
        }
        ControlRequest::Show { old, procedure } => {
            if let Some(procedure) = procedure {
                let result = invoke(
                    cpu,
                    mem,
                    api,
                    state,
                    log,
                    sched,
                    procedure,
                    &[a[0], 0x18, (a[1] != 0) as u32, 0],
                    max_instr,
                )?;
                if !matches!(result, GuestCallOutcome::Returned(_)) {
                    return Ok((result, "WM_SHOWWINDOW transferred".into()));
                }
            }
            value(old, "window visibility and paint state".into())
        }
        ControlRequest::Destroy => {
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
    }
}
#[allow(clippy::too_many_arguments)]
fn default(
    id: W,
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    let value = |value, note: String| Ok((GuestCallOutcome::Returned(value), note));
    match state
        .user32
        .prepare_default(id.wide(), a, mem, &mut state.graphics)?
    {
        DefaultRequest::Value(n, note) => value(n, note),
        DefaultRequest::Destroy => {
            control(W::Destroy, a, cpu, mem, api, state, log, sched, max_instr)
        }
        DefaultRequest::Position => super::window_geometry::default_windowpos_changed(
            a, cpu, mem, api, state, log, sched, max_instr,
        ),
        DefaultRequest::Paint => {
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
            value(0, "default paint validated".into())
        }
    }
}
