//! Address waiters share the scheduler's saved calls and callback scopes.
use super::{
    address_wait::AddressWait, call::Call, catalog::AddressWaitApi as A, context, ServiceError,
};
use crate::{
    machine::State,
    memory::Memory,
    process::{threads::PendingKind, Loaded},
};

fn error(state: &State, memory: &mut Memory, code: u32) -> Result<Option<u64>, ServiceError> {
    memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
    Ok(Some(0))
}

pub(super) fn uses_clock(id: A, state: &State, memory: &Memory, image: &Loaded) -> bool {
    let now = image.objects.clock.0;
    if let Some(p) = image.pending().filter(|p| p.pc == state.ip) {
        return matches!(&p.kind, PendingKind::Address(w) if w.deadline.is_some() && !w.ready(now));
    }
    if id != A::Wait {
        return super::call::peek_slot(state, memory, 0)
            .is_some_and(|p| image.threads.address_clock(p, now));
    }
    if !super::call::peek_slot(state, memory, 2).is_some_and(|s| matches!(s, 1 | 2 | 4 | 8)) {
        return false;
    }
    super::call::peek_slot(state, memory, 3).is_some_and(|t| t as u32 != 0 && t as u32 != u32::MAX)
}

pub(super) fn dispatch(
    id: A,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: &mut Loaded,
    sample: u64,
) -> Result<Option<u64>, ServiceError> {
    image.objects.clock.0 = image.objects.clock.0.max(sample);
    let now = image.objects.clock.0;
    let a = &call.arguments;
    if id != A::Wait {
        image.threads.wake_address(a[0], now, id == A::WakeAll);
        return Ok(Some(0));
    }
    if !matches!(a[2], 1 | 2 | 4 | 8) {
        return error(state, memory, 87);
    }
    if let Some(wait) = image
        .pending()
        .filter(|p| p.pc == state.ip)
        .and_then(|p| match &p.kind {
            PendingKind::Address(w) => Some(w),
            _ => None,
        })
    {
        if wait.address != a[0] {
            return Err(ServiceError::Unavailable(
                "address wait identity changed while blocked",
            ));
        }
        if wait.notified {
            return Ok(Some(1));
        }
        return if wait.expired(now) {
            error(state, memory, 1460)
        } else {
            Ok(None)
        };
    }
    if memory.read_int(a[0], a[2] as usize)? != memory.read_int(a[1], a[2] as usize)? {
        return Ok(Some(1));
    }
    let timeout = a[3] as u32;
    if timeout == 0 {
        return error(state, memory, 1460);
    }
    let ticket = image.threads.next_address;
    let next = ticket
        .checked_add(1)
        .ok_or(ServiceError::Unavailable("address wait sequence exhausted"))?;
    let wait = AddressWait {
        address: a[0],
        ticket,
        notified: false,
        deadline: (timeout != u32::MAX).then(|| now.saturating_add(u64::from(timeout) * 10000)),
    };
    image.set_pending(call.clone(), PendingKind::Address(wait), state.ip);
    image.threads.next_address = next;
    Ok(None)
}
