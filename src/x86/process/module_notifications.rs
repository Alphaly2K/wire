//! PE32 DLL/TLS notifications execute in the owning guest thread.
//! Module metadata remains private to Modules; snapshots preserve reentry rules.
use super::GuestCallOutcome;
use crate::{
    api::{ApiLogger, ApiState, ApiTable},
    cpu::Cpu,
    mem::GuestMem,
    modules::{Failure, LoadResult},
    thread::Sched,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn invoke_tls(
    handle: u32,
    reason: u32,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> LoadResult<()> {
    let callbacks = state.modules.tls_callbacks(handle);
    for function in callbacks {
        if !state.modules.image_present(handle) {
            break;
        }
        log.note(&format!(
            "TLS callback module={handle:#x} entry={function:#x} reason={reason}"
        ));
        match super::call_guest(
            cpu,
            mem,
            api,
            state,
            log,
            sched,
            function,
            &[handle, reason, 0],
            max_instr,
        )
        .map_err(Failure::Fatal)?
        {
            GuestCallOutcome::Returned(_) => {} // TLS callbacks return void.
            outcome => return Err(Failure::Control(outcome)),
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn notification(
    handle: u32,
    reason: u32,
    reserved: u32,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> LoadResult<()> {
    invoke_tls(handle, reason, cpu, mem, api, state, log, sched, max_instr)?;
    let entry = state.modules.notification_entry(handle);
    if entry != 0 {
        match super::call_guest(
            cpu,
            mem,
            api,
            state,
            log,
            sched,
            entry,
            &[handle, reason, reserved],
            max_instr,
        )
        .map_err(Failure::Fatal)?
        {
            GuestCallOutcome::Returned(_) => {}
            outcome => return Err(Failure::Control(outcome)),
        }
    }
    Ok(())
}
fn notification_result(result: LoadResult<()>) -> Result<GuestCallOutcome, String> {
    match result {
        Ok(()) => Ok(GuestCallOutcome::Returned(1)),
        Err(Failure::Control(outcome)) => Ok(outcome),
        Err(Failure::Fatal(message) | Failure::Expected(_, message)) => Err(message),
    }
}
/// Execute notifications in the owning thread, after TLS storage exists and
/// before its entry, or before releasing that storage on orderly thread exit.
#[allow(clippy::too_many_arguments)]
pub fn notify_thread(
    main: bool,
    attach: bool,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<GuestCallOutcome, String> {
    if state.modules.process_detaching() {
        return Ok(GuestCallOutcome::Returned(1));
    }
    let reason = if attach {
        if main {
            1
        } else {
            2
        }
    } else {
        3
    };
    let result = (|| -> LoadResult<()> {
        if !main || !attach {
            let mut order = state.modules.notification_order();
            if !attach {
                order.reverse();
            }
            for handle in order {
                if handle == state.exe_base {
                    continue;
                }
                let eligible = state.modules.thread_notification_enabled(handle);
                if eligible {
                    notification(
                        handle, reason, 0, cpu, mem, api, state, log, sched, max_instr,
                    )?;
                }
            }
        }
        invoke_tls(
            state.exe_base,
            reason,
            cpu,
            mem,
            api,
            state,
            log,
            sched,
            max_instr,
        )
    })();
    notification_result(result)
}
#[allow(clippy::too_many_arguments)]
pub fn notify_process_exit(
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> Result<GuestCallOutcome, String> {
    if state.modules.process_detaching() {
        return Ok(GuestCallOutcome::Returned(1));
    }
    state.modules.begin_process_detach();
    let order = state.modules.notification_order();
    let result = (|| -> LoadResult<()> {
        for handle in order.into_iter().rev() {
            if state.modules.notification_ready(handle) {
                notification(handle, 0, 1, cpu, mem, api, state, log, sched, max_instr)?;
            }
        }
        Ok(())
    })();
    notification_result(result)
}
