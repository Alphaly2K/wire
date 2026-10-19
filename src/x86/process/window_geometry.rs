/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; see COPYING.LIB. Adapted from Wine window/control flow.
 * Default WM_MOVE/WM_SIZE flow also follows Hangover Wine win32u/defwnd.c:
 * Copyright 1995 Alex Korobka; 2022 Jacek Caban for CodeWeavers.
 */
//! Execute callbacks between USER's geometry preparation and commit steps.
use super::{callbacks::invoke, GuestCallOutcome};
use crate::{
    api::{ApiLogger, ApiState, ApiTable, Args},
    cpu::Cpu,
    mem::GuestMem,
    thread::Sched,
    user32::WindowRequest,
    win32::catalog::WindowControlApi,
};
type ResultValue = Result<(GuestCallOutcome, String), String>;

#[allow(clippy::too_many_arguments)]
pub(crate) fn call(
    id: WindowControlApi,
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    let request = state.window_geometry_request(id, a, mem)?;
    execute(request, cpu, mem, api, state, log, sched, max_instr)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update(
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    let request = state.user32.prepare_update(a, &mut state.last_error)?;
    execute(request, cpu, mem, api, state, log, sched, max_instr)
}

#[allow(clippy::too_many_arguments)]
fn execute(
    request: WindowRequest,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    let request = if let WindowRequest::Position(change) = request {
        if change.notify {
            let result = invoke(
                cpu,
                mem,
                api,
                state,
                log,
                sched,
                change.procedure,
                &[change.hwnd, 0x46, 0, change.buffer],
                max_instr,
            )?;
            if !matches!(result, GuestCallOutcome::Returned(_)) {
                return Ok((result, "WM_WINDOWPOSCHANGING transferred".into()));
            }
        }
        state
            .user32
            .commit_position(change, mem, &mut state.last_error)?
    } else {
        request
    };
    match request {
        WindowRequest::Value(n, note) => Ok((GuestCallOutcome::Returned(n), note)),
        WindowRequest::Notify {
            procedure,
            args,
            value,
            note,
            transfer,
        } => {
            let result = invoke(
                cpu, mem, api, state, log, sched, procedure, &args, max_instr,
            )?;
            if !matches!(result, GuestCallOutcome::Returned(_)) {
                return Ok((result, transfer.into()));
            }
            Ok((GuestCallOutcome::Returned(value), note.into()))
        }
        WindowRequest::Position(_) => unreachable!("position is committed before notification"),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn default_windowpos_changed(
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    let Some(position) = state.user32.prepare_windowpos(a, mem)? else {
        return Ok((
            GuestCallOutcome::Returned(0),
            "window no longer exists".into(),
        ));
    };
    if let Some((procedure, args)) = position.movement {
        let result = invoke(
            cpu, mem, api, state, log, sched, procedure, &args, max_instr,
        )?;
        if !matches!(result, GuestCallOutcome::Returned(_)) {
            return Ok((result, "WM_MOVE transferred".into()));
        }
    }
    if let Some((procedure, args)) = state.user32.size_notification(&position) {
        let result = invoke(
            cpu, mem, api, state, log, sched, procedure, &args, max_instr,
        )?;
        if !matches!(result, GuestCallOutcome::Returned(_)) {
            return Ok((result, "WM_SIZE transferred".into()));
        }
    }
    Ok((
        GuestCallOutcome::Returned(0),
        "default WM_MOVE/WM_SIZE".into(),
    ))
}
