/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 * LGPL-2.1-or-later; see COPYING.LIB. Adapted from the Wine 10.0
 * window-text message flow previously in user32_common.rs.
 */
//! Process executes the notifications prepared by USER, including reentry.
use crate::{
    api::{ApiLogger, ApiState, ApiTable, Args},
    cpu::Cpu,
    mem::GuestMem,
    thread::Sched,
    user32::WindowStateRequest,
    win32::catalog::WindowStateApi,
};

use super::{callbacks::invoke, GuestCallOutcome};

#[allow(clippy::too_many_arguments)]
pub(crate) fn call(
    id: WindowStateApi,
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<(GuestCallOutcome, String), String> {
    match state
        .user32
        .prepare_window_state(id, a, sched.current_tid(), &mut state.last_error)
    {
        WindowStateRequest::Value(value, note) => Ok((GuestCallOutcome::Returned(value), note)),
        WindowStateRequest::Text { procedure, message } => {
            let outcome = invoke(
                cpu, mem, api, state, log, sched, procedure, &message, max_instr,
            )?;
            Ok((outcome, "window/control message callback".into()))
        }
        WindowStateRequest::Notifications {
            value,
            messages,
            count,
        } => {
            for message in messages.into_iter().take(count) {
                // Each preceding callback can destroy the next window or replace its procedure.
                if let Some(procedure) = state.user32.windows.get(&message[0]).map(|w| w.wndproc) {
                    let outcome = invoke(
                        cpu, mem, api, state, log, sched, procedure, &message, max_instr,
                    )?;
                    if !matches!(outcome, GuestCallOutcome::Returned(_)) {
                        return Ok((outcome, "window state callback transferred".into()));
                    }
                }
            }
            Ok((
                GuestCallOutcome::Returned(value),
                "logical input ownership".into(),
            ))
        }
    }
}
