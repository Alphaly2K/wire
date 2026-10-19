//! Callback execution and caller-context restoration belong to the process boundary.
use crate::{
    api::{ApiLogger, ApiState, ApiTable},
    cpu::Cpu,
    mem::GuestMem,
    thread::Sched,
};

use super::GuestCallOutcome;

#[allow(clippy::too_many_arguments)]
pub(crate) fn invoke(
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    proc: u32,
    a: &[u32],
    max_instr: u64,
) -> Result<GuestCallOutcome, String> {
    let saved = cpu.save_ctx();
    let result = super::call_guest(cpu, mem, api, state, log, sched, proc, a, max_instr);
    if matches!(result, Ok(GuestCallOutcome::Returned(_)) | Err(_)) {
        cpu.load_ctx(&saved);
    }
    result
}
