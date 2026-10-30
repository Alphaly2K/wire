//! Inline synchronization uses guest tokens and scoped cooperative waits.
use super::{
    call::Call,
    catalog::{CriticalSectionApi as C, InlineSyncApi as I},
    condition::{Mode as LockMode, Wait},
    context, critical_sections,
    srw::{Decision, Operation, Update},
    ServiceError,
};
use crate::{
    machine::{Mode, State},
    memory::{Memory, READ, WRITE},
    process::{threads::PendingKind, Loaded},
};

fn error(state: &State, memory: &mut Memory, code: u64) -> Result<Option<u64>, ServiceError> {
    memory.write_int(context::error_address(state, memory.mode()), 4, code)?;
    Ok(Some(0))
}
fn prepare_srw(
    image: &Loaded,
    memory: &Memory,
    pointer: u64,
    operation: Operation,
) -> Result<Decision, ServiceError> {
    let ptr = memory.mode() as usize / 8;
    memory.check(pointer, ptr, WRITE)?;
    let token = if matches!(operation, Operation::Initialize) {
        0
    } else {
        memory.read_int(pointer, ptr)?
    };
    let decision = image
        .srw
        .prepare(pointer, token, u64::from(image.threads.active), operation)
        .map_err(|_| ServiceError::Unavailable("invalid SRW ownership, token or reader count"))?;
    if let Decision::Update(update) = &decision {
        if memory.mode() == Mode::X86 && update.token() > u64::from(u32::MAX) {
            return Err(ServiceError::Unavailable("PE32 SRW reader count overflow"));
        }
    }
    Ok(decision)
}
fn publish_srw(
    image: &mut Loaded,
    memory: &mut Memory,
    pointer: u64,
    update: Update,
) -> Result<(), ServiceError> {
    memory.write_int(pointer, memory.mode() as usize / 8, update.token())?;
    image.srw.commit(update);
    Ok(())
}
fn block(image: &mut Loaded, call: &Call, state: &State) -> Option<u64> {
    if image.pending().is_none_or(|p| p.pc != state.ip) {
        image.set_pending(call.clone(), PendingKind::Inline, state.ip);
    }
    None
}
fn critical_release(memory: &Memory, pointer: u64, thread: u32) -> Result<(), ServiceError> {
    let ptr = memory.mode() as usize / 8;
    memory.check(pointer, if ptr == 4 { 24 } else { 40 }, READ)?;
    let count = memory.read_int(pointer + ptr as u64 + 4, 4)?;
    let owner = memory.read_int(pointer + ptr as u64 + 8, ptr)?;
    if count != 1 || owner != u64::from(thread) {
        return Err(ServiceError::Unavailable(
            "condition wait requires one owned critical-section entry",
        ));
    }
    memory.check(pointer + ptr as u64, ptr + 8, WRITE)?;
    Ok(())
}
fn critical_call(
    id: C,
    lock: u64,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    thread: u32,
) -> Result<Option<u64>, ServiceError> {
    let mut lock_call = call.clone();
    lock_call.arguments[0] = lock;
    critical_sections::dispatch(id, &lock_call, state, memory, thread)
}
fn condition_wait(
    critical: bool,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: &mut Loaded,
    now: u64,
) -> Result<Option<u64>, ServiceError> {
    let a = &call.arguments;
    let flags = if critical { 0 } else { a[3] as u32 };
    let mode = if critical {
        LockMode::Critical
    } else {
        LockMode::Srw(flags == 1)
    };
    let pending = image
        .pending()
        .filter(|p| p.pc == state.ip)
        .and_then(|p| match &p.kind {
            PendingKind::Condition(w) => Some(w.clone()),
            _ => None,
        });
    if let Some(wait) = pending {
        if wait.condition != a[0]
            || wait.lock != a[1]
            || wait.timeout != a[2] as u32
            || wait.mode != mode
            || flags > 1
        {
            return Err(ServiceError::Unavailable(
                "condition wait identity changed while blocked",
            ));
        }
        if !wait.notified && !wait.expired(now) {
            return Ok(None);
        }
        if !wait.notified {
            memory.check(context::error_address(state, memory.mode()), 4, WRITE)?;
        }
        match wait.mode {
            LockMode::Critical => {
                if critical_call(
                    C::Enter,
                    wait.lock,
                    call,
                    state,
                    memory,
                    image.threads.active,
                )?
                .is_none()
                {
                    return Ok(None);
                }
            }
            LockMode::Srw(shared) => {
                let Decision::Update(update) =
                    prepare_srw(image, memory, wait.lock, Operation::Acquire { shared })?
                else {
                    return Ok(None);
                };
                publish_srw(image, memory, wait.lock, update)?;
            }
        }
        if !wait.notified {
            return error(state, memory, 1460);
        }
        return Ok(Some(1));
    }
    if flags > 1 {
        return error(state, memory, 87);
    }
    let ptr = memory.mode() as usize / 8;
    if memory.read_int(a[0], ptr)? != 0 {
        return Err(ServiceError::Unavailable(
            "unsupported condition-variable wait-list token",
        ));
    }
    let release = if critical {
        critical_release(memory, a[1], image.threads.active)?;
        None
    } else {
        let Decision::Update(update) = prepare_srw(
            image,
            memory,
            a[1],
            Operation::Release { shared: flags == 1 },
        )?
        else {
            unreachable!("SRW release cannot contend");
        };
        Some(update)
    };
    let timeout = a[2] as u32;
    if timeout == 0 {
        // No other guest can observe a release/reacquire within this call.
        return error(state, memory, 1460);
    }
    let ticket = image.threads.next_condition;
    let next = ticket.checked_add(1).ok_or(ServiceError::Unavailable(
        "condition wait sequence exhausted",
    ))?;
    let wait = Wait {
        condition: a[0],
        lock: a[1],
        mode,
        timeout,
        ticket,
        notified: false,
        deadline: (timeout != u32::MAX).then(|| now.saturating_add(u64::from(timeout) * 10000)),
    };
    // Queue admission and all memory checks precede releasing the lock.
    if let Some(update) = release {
        publish_srw(image, memory, a[1], update)?;
    } else {
        critical_call(C::Leave, a[1], call, state, memory, image.threads.active)?;
    }
    image.threads.next_condition = next;
    image.set_pending(call.clone(), PendingKind::Condition(wait), state.ip);
    image.threads.changed();
    Ok(None)
}

pub(super) fn uses_clock(id: I, state: &State, memory: &Memory, image: &Loaded) -> bool {
    let now = image.objects.clock.0;
    if let Some(p) = image.pending().filter(|p| p.pc == state.ip) {
        return matches!(&p.kind, PendingKind::Condition(w) if w.deadline.is_some() && !w.notified && !w.expired(now));
    }
    match id {
        I::SleepConditionCs | I::SleepConditionSrw => super::call::peek_slot(state, memory, 2)
            .is_some_and(|t| t as u32 != 0 && t as u32 != u32::MAX),
        I::WakeCondition | I::WakeAllCondition => super::call::peek_slot(state, memory, 0)
            .is_some_and(|p| image.threads.condition_clock(p, now)),
        _ => false,
    }
}

pub(super) fn dispatch(
    id: I,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: &mut Loaded,
    sample: u64,
) -> Result<Option<u64>, ServiceError> {
    image.objects.clock.0 = image.objects.clock.0.max(sample);
    let now = image.objects.clock.0;
    let a = &call.arguments;
    let ptr = memory.mode() as usize / 8;
    let value = match id {
        I::SleepConditionCs | I::SleepConditionSrw => {
            return condition_wait(id == I::SleepConditionCs, call, state, memory, image, now)
        }
        I::InitializeCondition => {
            if image.threads.has_condition(a[0]) {
                return Err(ServiceError::Unavailable(
                    "cannot reinitialize a condition variable with active waits",
                ));
            }
            memory.write_int(a[0], ptr, 0)?;
            0
        }
        I::WakeCondition | I::WakeAllCondition => {
            if memory.read_int(a[0], ptr)? != 0 {
                return Err(ServiceError::Unavailable(
                    "unsupported condition-variable wait-list token",
                ));
            }
            image
                .threads
                .wake_condition(a[0], now, id == I::WakeAllCondition);
            0
        }
        I::InitializeSrw
        | I::AcquireExclusive
        | I::AcquireShared
        | I::ReleaseExclusive
        | I::ReleaseShared
        | I::TryExclusive
        | I::TryShared => {
            let shared = matches!(id, I::AcquireShared | I::ReleaseShared | I::TryShared);
            let attempt = matches!(id, I::TryExclusive | I::TryShared);
            let operation = match id {
                I::InitializeSrw => Operation::Initialize,
                I::ReleaseExclusive | I::ReleaseShared => Operation::Release { shared },
                _ => Operation::Acquire { shared },
            };
            match prepare_srw(image, memory, a[0], operation)? {
                Decision::Contended => {
                    return Ok(if attempt {
                        Some(0)
                    } else {
                        block(image, call, state)
                    })
                }
                Decision::Update(update) => publish_srw(image, memory, a[0], update)?,
            }
            u64::from(attempt)
        }
        I::BeginOnce => {
            if a[1] & !1 != 0 {
                return error(state, memory, if a[1] & !3 == 0 { 50 } else { 87 });
            }
            let token = memory.read_int(a[0], ptr)?;
            memory.check(a[2], 4, WRITE)?;
            if a[3] != 0 {
                memory.check(a[3], ptr, WRITE)?;
            }
            if token & 3 == 2 {
                memory.write_int(a[2], 4, 0)?;
                if a[3] != 0 {
                    memory.write_int(a[3], ptr, token & !3)?;
                }
                1
            } else if a[1] & 1 != 0 {
                return error(state, memory, 31);
            } else if token == 0 {
                memory.check(a[0], ptr, WRITE)?;
                memory.write_int(a[0], ptr, 1)?;
                memory.write_int(a[2], 4, 1)?;
                if a[3] != 0 {
                    memory.write_int(a[3], ptr, 0)?;
                }
                1
            } else if token == 1 {
                return Ok(block(image, call, state));
            } else {
                return error(state, memory, 87);
            }
        }
        I::CompleteOnce => {
            if a[1] & !4 != 0 {
                return error(state, memory, if a[1] == 2 { 50 } else { 87 });
            }
            let failed = a[1] & 4 != 0;
            if a[2] & 3 != 0 || failed && a[2] != 0 || memory.read_int(a[0], ptr)? != 1 {
                return error(state, memory, 87);
            }
            memory.write_int(a[0], ptr, if failed { 0 } else { a[2] | 2 })?;
            1
        }
    };
    Ok(Some(value))
}
