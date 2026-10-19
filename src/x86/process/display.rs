//! Monitor callbacks execute here; the display adapter prepares checked inputs.
use super::{callbacks::invoke, GuestCallOutcome};
use crate::{
    api::{ApiLogger, ApiState, ApiTable, Args},
    cpu::Cpu,
    mem::GuestMem,
    thread::Sched,
    win32::x86::display::{prepare_monitors, MonitorRequest},
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn enumerate_monitors(
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<(GuestCallOutcome, String), String> {
    match prepare_monitors(
        a,
        mem,
        &mut state.heap,
        state.process_heap,
        &mut state.last_error,
    )? {
        MonitorRequest::Value(value) => Ok((GuestCallOutcome::Returned(value.value), value.note)),
        MonitorRequest::Callback {
            procedure,
            args,
            allocation,
        } => {
            let result = invoke(
                cpu, mem, api, state, log, sched, procedure, &args, max_instr,
            );
            state.heap.free(state.process_heap, allocation);
            result.map(|value| (value, "enumerated the logical 1024x768 monitor".into()))
        }
    }
}
