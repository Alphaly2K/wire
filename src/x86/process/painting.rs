//! Paint callbacks run at the process boundary after USER/DC preparation.
use super::{callbacks::invoke, GuestCallOutcome};
use crate::{
    api::{ApiLogger, ApiState, ApiTable, Args},
    cpu::Cpu,
    mem::GuestMem,
    thread::Sched,
    user32::{PaintRequest, PaintingOperation},
    win32::catalog::PaintingApi,
};
type ResultValue = Result<(GuestCallOutcome, String), String>;

#[allow(clippy::too_many_arguments)]
pub(crate) fn call(
    id: PaintingApi,
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    execute(
        PaintingOperation::Paint(id),
        a,
        cpu,
        mem,
        api,
        state,
        log,
        sched,
        max_instr,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn redraw(
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    execute(
        PaintingOperation::Redraw,
        a,
        cpu,
        mem,
        api,
        state,
        log,
        sched,
        max_instr,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute(
    operation: PaintingOperation,
    a: &Args,
    cpu: &mut Cpu,
    mem: &mut GuestMem,
    api: &mut ApiTable,
    state: &mut ApiState,
    log: &mut ApiLogger,
    sched: &mut Sched,
    max_instr: u64,
) -> ResultValue {
    let value = |n| {
        Ok((
            GuestCallOutcome::Returned(n),
            "window update/paint state".into(),
        ))
    };
    match state.user32.prepare_paint(
        operation,
        a,
        mem,
        &mut state.graphics,
        &mut state.last_error,
    )? {
        PaintRequest::Value(n) => value(n),
        PaintRequest::Update => crate::user32::control_call(
            crate::win32::catalog::WindowControlApi::Update,
            a,
            cpu,
            mem,
            api,
            state,
            log,
            sched,
            max_instr,
        ),
        PaintRequest::Begin(begin) => {
            let mut erased = !begin.erase;
            if begin.erase {
                let result = invoke(
                    cpu,
                    mem,
                    api,
                    state,
                    log,
                    sched,
                    begin.procedure,
                    &begin.args,
                    max_instr,
                )?;
                match result {
                    GuestCallOutcome::Returned(n) => erased = n != 0,
                    _ => return Ok((result, "WM_ERASEBKGND transferred".into())),
                }
            }
            value(begin.finish(mem, erased)?)
        }
    }
}
