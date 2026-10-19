//! Execute received cross-thread sends at a guest scheduling boundary.
use super::{callbacks::invoke, GuestCallOutcome};
use crate::{
    api::{ApiLogger, ApiState, ApiTable},
    cpu::Cpu,
    mem::GuestMem,
    thread::Sched,
};

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
