/*
 * Copyright 1993, 1994 Alexandre Julliard
 *
 * This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * This library is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
 * Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with this library; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin St, Fifth Floor, Boston, MA 02110-1301, USA
 */

//! Wine 10.0 user32/win.c enumeration flow, adapted to the wire window table
//! and guest callbacks. Snapshot the list, skip windows destroyed by callbacks,
//! stop on callback FALSE. Presentation and complete z-order remain separate.
use crate::api::{ApiLogger, ApiState, ApiTable};
use crate::cpu::{Cpu, GuestCallOutcome};
use crate::mem::GuestMem;
use crate::thread::Sched;

#[allow(clippy::too_many_arguments)]
pub fn enumerate(
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
    let (tid, callback, param) = if name == "EnumThreadWindows" {
        (a[0], a[1], a[2])
    } else {
        (0, a[0], a[1])
    };
    if callback == 0 {
        state.last_error = 87;
        return Ok((
            GuestCallOutcome::Returned(0),
            "null enumeration callback".into(),
        ));
    }
    let windows: Vec<_> = state
        .user32
        .windows
        .iter()
        .rev()
        .filter(|(_, w)| {
            w.style & 0x40000000 == 0 && w.parent != 0xfffffffd && (tid == 0 || tid == w.tid)
        })
        .map(|(&h, _)| h)
        .collect();
    let mut returned = 1;
    let mut visited = 0;
    for hwnd in windows {
        if !state.user32.windows.contains_key(&hwnd) {
            continue;
        }
        visited += 1;
        match crate::user32::invoke(
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
