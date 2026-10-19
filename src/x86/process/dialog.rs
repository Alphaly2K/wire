/*
 * Copyright 1993, 1994, 1996 Alexandre Julliard
 *
 * This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public License
 * as published by the Free Software Foundation; either version 2.1
 * of the License, or (at your option) any later version.
 * This library is distributed without any warranty; see COPYING.LIB.
 */
//! Execute USER control requests at the process callback boundary.
use super::{callbacks::invoke, GuestCallOutcome};
use crate::{
    api::{ApiLogger, ApiState, ApiTable, Args},
    cpu::Cpu,
    mem::GuestMem,
    thread::Sched,
    user32::{dialog_integer, DialogRequest},
    win32::catalog::DialogApi,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn call(
    id: DialogApi,
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<(GuestCallOutcome, String), String> {
    match state.dialog_request(id, a, mem, sched.current_tid())? {
        DialogRequest::Value(value, note) => Ok((GuestCallOutcome::Returned(value), note)),
        DialogRequest::Message {
            procedure,
            args,
            output,
        } => {
            let result = invoke(
                cpu, mem, api, state, log, sched, procedure, &args, max_instr,
            )?;
            if let (Some(p), GuestCallOutcome::Returned(count)) = (output, &result) {
                let value = dialog_integer(a, mem, p, *count)?;
                return Ok((
                    GuestCallOutcome::Returned(value),
                    "dialog decimal conversion".into(),
                ));
            }
            Ok((result, "window/control message callback".into()))
        }
        DialogRequest::Enumeration {
            windows,
            callback,
            param,
            children: true,
        } => {
            for h in windows {
                if !state.user32.windows.contains_key(&h) {
                    continue;
                }
                let result = invoke(
                    cpu,
                    mem,
                    api,
                    state,
                    log,
                    sched,
                    callback,
                    &[h, param],
                    max_instr,
                )?;
                match result {
                    GuestCallOutcome::Returned(0) => break,
                    GuestCallOutcome::Returned(_) => {}
                    _ => return Ok((result, "child enumeration transferred".into())),
                }
            }
            Ok((
                GuestCallOutcome::Returned(1),
                "child window snapshot".into(),
            ))
        }
        DialogRequest::Enumeration {
            windows,
            callback,
            param,
            children: false,
        } => {
            let mut returned = 1;
            let mut visited = 0;
            for hwnd in windows {
                if !state.user32.windows.contains_key(&hwnd) {
                    continue;
                }
                visited += 1;
                match invoke(
                    cpu,
                    mem,
                    api,
                    state,
                    log,
                    sched,
                    callback,
                    &[hwnd, param],
                    max_instr,
                )? {
                    GuestCallOutcome::Returned(value) => {
                        returned = value;
                        if value == 0 {
                            break;
                        }
                    }
                    outcome => {
                        return Ok((
                            outcome,
                            "window enumeration callback transferred control".into(),
                        ))
                    }
                }
            }
            Ok((
                GuestCallOutcome::Returned(returned),
                format!("enumerated {visited} windows"),
            ))
        }
    }
}
