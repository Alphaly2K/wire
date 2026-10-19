//! Orderly exits belong to process orchestration because DLL notifications may re-enter guest code.
use crate::{
    api::{ApiLogger, ApiState, ApiTable},
    cpu::Cpu,
    mem::GuestMem,
    thread::Sched,
};

use super::GuestCallOutcome;

#[allow(clippy::too_many_arguments)]
pub(crate) fn exit_thread(
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
    code: u32,
) -> Result<(), String> {
    match crate::modules::notify_thread(false, false, cpu, mem, api, state, log, sched, max_instr)?
    {
        GuestCallOutcome::Returned(_) => {}
        _ => return Ok(()),
    }
    state
        .modules
        .release_tls_thread(&mut state.heap, state.process_heap, cpu.fs_base);
    state.user32.exit_thread(
        &mut state.graphics,
        &mut state.menus,
        sched,
        sched.current_tid(),
    );
    sched.exit_current(cpu, code)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn exit_process(
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
    code: u32,
) -> Result<(), String> {
    match crate::modules::notify_process_exit(cpu, mem, api, state, log, sched, max_instr)? {
        GuestCallOutcome::Returned(_) => {}
        _ => return Ok(()),
    }
    cpu.halted = true;
    cpu.exit_code = Some(code);
    Ok(())
}
