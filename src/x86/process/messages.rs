//! Execute message waits, dispatch callbacks and received cross-thread sends.
use super::{callbacks::invoke, GuestCallOutcome};
use crate::{
    api::{ApiLogger, ApiState, ApiTable, Args},
    cpu::Cpu,
    mem::GuestMem,
    thread::Sched,
    user32::MessageRequest,
    win32::catalog::MessageApi,
};
use std::time::{Duration, Instant};

#[allow(clippy::too_many_arguments)]
pub(crate) fn call(
    id: MessageApi,
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
        .prepare_message(id, a, mem, &mut state.last_error, sched, state.start)?
    {
        MessageRequest::Value(value, note) => Ok((GuestCallOutcome::Returned(value), note)),
        MessageRequest::Wait { tid, mut deadline } => {
            if state.presenter.active() {
                let ui = Instant::now() + Duration::from_millis(16);
                deadline = Some(deadline.map_or(ui, |d| d.min(ui)));
            }
            sched.block_current(cpu, crate::thread::WaitTarget::Messages(tid), deadline)?;
            Ok((
                GuestCallOutcome::Transferred,
                "waiting for a message".into(),
            )) // redispatch this same stub when woken
        }
        MessageRequest::Dispatch {
            procedure,
            args,
            note,
        } => {
            let result = invoke(
                cpu, mem, api, state, log, sched, procedure, &args, max_instr,
            )?;
            Ok((result, note.into()))
        }
    }
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
        let Some(call) = state.user32.next_sent(tid) else {
            return Ok(None);
        };
        let result = if let Some(procedure) = call.procedure {
            invoke(
                cpu, mem, api, state, log, sched, procedure, &call.args, max_instr,
            )?
        } else {
            GuestCallOutcome::Returned(0)
        };
        let GuestCallOutcome::Returned(value) = result else {
            return Ok(Some(result));
        };
        state.user32.complete_sent(call.id, value, sched)?;
    }
}
