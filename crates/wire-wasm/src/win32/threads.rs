use super::{
    address_waits,
    call::{peek_slot, Call},
    catalog::{
        ApiId, Contract, DelayApi as D, MessageApi as M, ThreadControlApi as C,
        ThreadPropertyApi as P, WaitApi as V, WinsockApi as W,
    },
    context, critical_sections, inline_sync, messages, ServiceError, Services,
};
use crate::{
    machine::State,
    memory::Memory,
    process::{
        threads::{MessageWait, PendingKind, WaitTarget},
        LoadError, Loaded,
    },
};

pub(super) fn implemented(id: ApiId) -> bool {
    matches!(
        id,
        ApiId::ThreadControl(C::Create | C::Exit | C::Context | C::SetContext | C::QueueApc)
            | ApiId::CriticalSection(_)
            | ApiId::InlineSync(_)
            | ApiId::AddressWait(_)
            | ApiId::ThreadProperty(_)
            | ApiId::Delay(_)
            | ApiId::Wait(_)
            | ApiId::Winsock(W::WaitEvents)
            | ApiId::Message(M::WaitObjects | M::WaitObjectsEx)
    )
}

pub(super) fn uses_clock(
    id: ApiId,
    state: &State,
    memory: &Memory,
    image: Option<&Loaded>,
) -> bool {
    if let ApiId::InlineSync(id) = id {
        return image.is_some_and(|i| inline_sync::uses_clock(id, state, memory, i));
    }
    if let ApiId::AddressWait(id) = id {
        return image.is_some_and(|i| address_waits::uses_clock(id, state, memory, i));
    }
    if let Some(image) = image {
        if id == ApiId::Winsock(W::WaitEvents)
            && (!image.winsock.initialized()
                || image.pending().filter(|p| p.pc == state.ip).is_none()
                    && peek_slot(state, memory, 0).is_none_or(|n| n as u32 == 0 || n as u32 > 64))
        {
            return false;
        }
        if !image.threads.current().apcs.is_empty() {
            let alertable = image.pending().filter(|p| p.pc == state.ip).map_or_else(
                || {
                    let slot = match id {
                        ApiId::Wait(V::SignalAndWait) => Some((3, 1)),
                        ApiId::Delay(D::SleepEx) => Some((1, 1)),
                        ApiId::Wait(w) if w.alertable() => {
                            Some((if w.multiple() { 4 } else { 2 }, 1))
                        }
                        ApiId::Winsock(W::WaitEvents) => Some((4, 1)),
                        ApiId::Message(M::WaitObjectsEx) => Some((4, 2)),
                        _ => None,
                    };
                    slot.is_some_and(|(n, mask)| {
                        peek_slot(state, memory, n).is_some_and(|v| {
                            if mask == 1 {
                                v != 0
                            } else {
                                v & mask != 0
                            }
                        })
                    })
                },
                |p| {
                    matches!(
                        p.kind,
                        PendingKind::Wait {
                            alertable: true,
                            ..
                        } | PendingKind::Sleep {
                            alertable: true,
                            ..
                        }
                    )
                },
            );
            if alertable {
                return false;
            }
        }
    }
    if let Some(p) = image.and_then(|i| i.pending()).filter(|p| p.pc == state.ip) {
        if p.result.is_some() {
            return false;
        }
        return match &p.kind {
            PendingKind::FileInput => false,
            PendingKind::Message | PendingKind::Critical | PendingKind::Inline => false,
            PendingKind::Condition(w) => w.deadline.is_some() && !w.notified,
            PendingKind::Address(w) => w.deadline.is_some() && !w.notified,
            PendingKind::Sleep { deadline, .. } => deadline.is_some(),
            PendingKind::Wait {
                deadline,
                targets,
                messages,
                ..
            } => {
                deadline.is_some()
                    || image.is_some_and(|i| {
                        messages.is_some_and(|m| i.user.message_clock(i.threads.active, m.mask))
                    })
                    || image.is_some_and(|i| {
                        targets.iter().any(
                            |t| matches!(t, WaitTarget::Object(id) if i.objects.needs_time(*id)),
                        )
                    })
            }
        };
    }
    let timeout_slot = match id {
        ApiId::Wait(V::SignalAndWait) => 2,
        ApiId::Delay(D::Sleep | D::SleepEx) => 0,
        ApiId::Wait(id) => {
            if id.multiple() {
                3
            } else {
                1
            }
        }
        ApiId::Winsock(W::WaitEvents) | ApiId::Message(M::WaitObjects) => 3,
        ApiId::Message(M::WaitObjectsEx) => 2,
        _ => return false,
    };
    let timeout = peek_slot(state, memory, timeout_slot).unwrap_or(0) as u32;
    if timeout != 0 && timeout != u32::MAX {
        return true;
    }
    let Some(image) = image else {
        return false;
    };
    if matches!(id, ApiId::Message(_)) {
        let mask_slot = if id == ApiId::Message(M::WaitObjects) {
            4
        } else {
            3
        };
        if peek_slot(state, memory, mask_slot)
            .is_some_and(|m| image.user.message_clock(image.threads.active, m as u32))
        {
            return true;
        }
    }
    match id {
        ApiId::Message(M::WaitObjects | M::WaitObjectsEx)
        | ApiId::Winsock(W::WaitEvents)
        | ApiId::Wait(_)
            if !matches!(id,ApiId::Wait(w) if !w.multiple()) =>
        {
            let count = peek_slot(state, memory, 0).unwrap_or(0) as u32;
            if count == 0 || count > 64 {
                return false;
            }
            let Some(address) = peek_slot(state, memory, 1) else {
                return false;
            };
            let ptr = memory.mode() as usize / 8;
            let mut bytes = vec![0; count as usize * ptr];
            if memory.observe(address, &mut bytes).is_err() {
                return false;
            }
            bytes.chunks_exact(ptr).any(|chunk| {
                let mut word = [0; 8];
                word[..ptr].copy_from_slice(chunk);
                image.objects.handle_needs_time(u64::from_le_bytes(word))
            })
        }
        ApiId::Wait(_) => peek_slot(
            state,
            memory,
            if id == ApiId::Wait(V::SignalAndWait) {
                1
            } else {
                0
            },
        )
        .is_some_and(|h| image.objects.handle_needs_time(h)),
        _ => false,
    }
}

fn property(
    id: P,
    a: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, LoadError> {
    if id == P::Open && image.threads.get(a[2] as u32).is_none() {
        return image
            .process
            .primary_open(a[2] as u32, a[0] as u32, a[1] != 0)
            .map_err(LoadError::Windows);
    }
    if !matches!(
        id,
        P::Open | P::GetErrorMode | P::SetErrorMode | P::StackGuarantee
    ) {
        let access = match id {
            P::Suspend | P::Resume => 2,
            P::SetPriority | P::SetAffinity => 0x420,
            _ => 0x840,
        };
        if let Some(record) = image
            .process
            .primary(a[0], access)
            .map_err(LoadError::Windows)?
        {
            let result = match id {
                P::Id => record.first_thread().into(),
                P::ExitCode => {
                    memory.write_int(a[1], 4, record.primary_exit.get().unwrap_or(259).into())?;
                    1
                }
                P::GetPriority => record.priority.get() as u32 as u64,
                P::SetPriority => {
                    let priority = a[1] as u32 as i32;
                    if !matches!(priority, -15 | -2..=2 | 15) {
                        return Err(LoadError::Windows(87));
                    }
                    record.priority.set(priority);
                    1
                }
                P::SetAffinity => {
                    if a[1] != 1 {
                        return Err(LoadError::Windows(87));
                    }
                    1
                }
                P::Suspend | P::Resume => {
                    if record.primary_exit.get().is_some() {
                        return Err(LoadError::Windows(6));
                    }
                    let old = record.suspended.get();
                    if id == P::Suspend && old == 127 {
                        return Err(LoadError::Windows(87));
                    }
                    record.suspended.set(if id == P::Suspend {
                        old + 1
                    } else {
                        old.saturating_sub(1)
                    });
                    old.into()
                }
                _ => unreachable!(),
            };
            image.threads.changed();
            return Ok(result);
        }
    }
    let t = &mut image.threads;
    let query = 0x840;
    let set = 0x420;
    let result = match id {
        P::Open => t
            .open(a[2] as u32, a[0] as u32, a[1] != 0)
            .map_err(LoadError::Windows)?,
        P::GetErrorMode => u64::from(t.current().error_mode),
        P::SetErrorMode => {
            if a[0] as u32 & !0x8003 != 0 {
                return Err(LoadError::Windows(87));
            }
            if a[1] != 0 {
                memory.write_int(a[1], 4, u64::from(t.current().error_mode))?;
            }
            t.current_mut().error_mode = a[0] as u32;
            1
        }
        P::StackGuarantee => {
            let size = memory.read_int(a[0], 4)?;
            if size != 0 {
                return Err(LoadError::Windows(50));
            }
            memory.write_int(a[0], 4, 0)?;
            1
        }
        _ => {
            let access = match id {
                P::Suspend | P::Resume => 2,
                P::SetPriority | P::SetAffinity => set,
                _ => query,
            };
            let target = t.target(a[0], access).map_err(LoadError::Windows)?;
            let thread = t.get_mut(target);
            match id {
                P::Id => u64::from(target),
                P::ExitCode => {
                    memory.write_int(a[1], 4, u64::from(thread.exit.unwrap_or(259)))?;
                    1
                }
                P::GetPriority => thread.priority as u32 as u64,
                P::SetPriority => {
                    let priority = a[1] as u32 as i32;
                    if !matches!(priority, -15 | -2..=2 | 15) {
                        return Err(LoadError::Windows(87));
                    }
                    thread.priority = priority;
                    1
                }
                P::SetAffinity => {
                    if a[1] != 1 {
                        return Err(LoadError::Windows(87));
                    }
                    1
                }
                P::Suspend | P::Resume => {
                    if thread.exit.is_some() {
                        return Err(LoadError::Windows(6));
                    }
                    let old = thread.suspended;
                    if id == P::Suspend {
                        if old == 127 {
                            return Err(LoadError::Windows(87));
                        }
                        thread.suspended += 1;
                    } else {
                        thread.suspended = old.saturating_sub(1);
                    }
                    u64::from(old)
                }
                _ => unreachable!(),
            }
        }
    };
    if let Some(main) = t.primary() {
        image.process.record.suspended.set(main.suspended);
        image.process.record.priority.set(main.priority);
    }
    Ok(result)
}

pub(crate) fn wait_value(
    image: &mut Loaded,
    thread: u32,
    targets: &[WaitTarget],
    all: bool,
    deadline: Option<u64>,
    messages: Option<MessageWait>,
    now: u64,
) -> Result<Option<u64>, LoadError> {
    let value = {
        if let Some(m) = messages {
            image.user.refresh_messages(thread, now, m.mask);
        }
        let queued =
            messages.is_some_and(|m| image.user.messages.ready(thread, m.mask, m.input_available));
        let mut ready = Vec::with_capacity(targets.len());
        let mut abandoned = Vec::with_capacity(targets.len());
        for target in targets {
            match target {
                WaitTarget::Process(id) | WaitTarget::Primary(id) => {
                    let record = image.process.record(*id).ok_or(LoadError::Windows(6))?;
                    ready.push(if matches!(target, WaitTarget::Primary(_)) {
                        record.primary_exit.get().is_some()
                    } else {
                        record.exit.get().is_some()
                    });
                    abandoned.push(false);
                }
                WaitTarget::Thread(id) => {
                    ready.push(image.threads.get(*id).is_some_and(|t| t.exit.is_some()));
                    abandoned.push(false);
                }
                WaitTarget::Object(id) => {
                    let info = image
                        .objects
                        .peek_id(*id, thread)
                        .map_err(LoadError::Windows)?;
                    ready.push(info.ready);
                    abandoned.push(info.abandoned);
                }
            }
        }
        let selected = if all {
            (ready.iter().all(|r| *r) && (messages.is_none() || queued)).then_some(0)
        } else {
            ready
                .iter()
                .position(|r| *r)
                .or_else(|| queued.then_some(targets.len()))
        };
        if let Some(selected) = selected {
            let first_abandoned = if all {
                abandoned.iter().position(|a| *a)
            } else {
                abandoned
                    .get(selected)
                    .copied()
                    .unwrap_or(false)
                    .then_some(selected)
            };
            for (n, target) in targets.iter().enumerate() {
                if all || n == selected {
                    if let WaitTarget::Object(id) = target {
                        image.objects.acquire_id(*id, thread);
                    }
                }
            }
            Some(if first_abandoned.is_some() {
                128 + if all { 0 } else { selected as u64 }
            } else {
                selected as u64
            })
        } else if deadline.is_some_and(|d| now >= d) {
            Some(258)
        } else {
            None
        }
    };
    if value.is_some() {
        if let Some(m) = messages {
            image.user.messages.checked(thread, m.mask);
        }
    }
    Ok(value)
}

fn waiting(
    id: ApiId,
    call: &Call,
    state: &State,
    memory: &Memory,
    image: &mut Loaded,
    sample: u64,
) -> Result<Option<u64>, LoadError> {
    let a = &call.arguments;
    let winsock = id == ApiId::Winsock(W::WaitEvents);
    if winsock && !image.winsock.initialized() {
        return Err(LoadError::Windows(10093));
    }
    let pending = image.pending().filter(|p| p.pc == state.ip).cloned();
    if let Some(value) = pending.as_ref().and_then(|p| p.result) {
        return Ok(Some(value));
    }
    let retry = pending.is_some();
    image.objects.clock.0 = image.objects.clock.0.max(sample);
    let now = image.objects.clock.0;
    let kind = if let Some(pending) = pending {
        pending.kind
    } else {
        match id {
            ApiId::Wait(_)
            | ApiId::Winsock(W::WaitEvents)
            | ApiId::Message(M::WaitObjects | M::WaitObjectsEx) => {
                let (multiple, all, timeout, message, alertable) = match id {
                    ApiId::Wait(V::SignalAndWait) => (false, false, a[2] as u32, None, a[3] != 0),
                    ApiId::Wait(id) => (
                        id.multiple(),
                        a[2] != 0,
                        a[if id.multiple() { 3 } else { 1 }] as u32,
                        None,
                        id.alertable() && a[if id.multiple() { 4 } else { 2 }] != 0,
                    ),
                    ApiId::Winsock(W::WaitEvents) => {
                        (true, a[2] != 0, a[3] as u32, None, a[4] != 0)
                    }
                    ApiId::Message(id) => {
                        let ex = id == M::WaitObjectsEx;
                        let flags = if ex {
                            a[4] as u32
                        } else {
                            u32::from(a[2] != 0)
                        };
                        let mask = a[if ex { 3 } else { 4 }] as u32;
                        if flags & !7 != 0 || mask & !messages::MASK != 0 {
                            return Err(LoadError::Windows(87));
                        }
                        (
                            true,
                            flags & 1 != 0,
                            a[if ex { 2 } else { 3 }] as u32,
                            Some(MessageWait {
                                mask,
                                input_available: flags & 4 != 0,
                            }),
                            flags & 2 != 0,
                        )
                    }
                    _ => unreachable!(),
                };
                let handles = if multiple {
                    let count = a[0] as u32 as usize;
                    if count > if message.is_some() { 63 } else { 64 }
                        || count == 0 && message.is_none()
                    {
                        return Err(LoadError::Windows(if winsock { 10022 } else { 87 }));
                    }
                    let ptr = memory.mode() as usize / 8;
                    let mut bytes = vec![0; count * ptr];
                    if count != 0 {
                        memory.read(a[1], &mut bytes)?;
                    }
                    bytes
                        .chunks_exact(ptr)
                        .map(|chunk| {
                            let mut word = [0; 8];
                            word[..ptr].copy_from_slice(chunk);
                            u64::from_le_bytes(word)
                        })
                        .collect::<Vec<_>>()
                } else {
                    vec![a[usize::from(id == ApiId::Wait(V::SignalAndWait))]]
                };
                let mut targets = Vec::with_capacity(handles.len());
                for h in handles {
                    let target = if image.objects.flags(h).is_some() {
                        WaitTarget::Object(
                            image
                                .objects
                                .peek_for(h, image.threads.active)
                                .map_err(LoadError::Windows)?
                                .id,
                        )
                    } else if image.process.pseudo(h) || image.process.flags(h).is_some() {
                        if let Some(record) = image
                            .process
                            .primary(h, 0x100000)
                            .map_err(LoadError::Windows)?
                        {
                            WaitTarget::Primary(record.id)
                        } else {
                            image
                                .process
                                .check(h, crate::process::current::SYNCHRONIZE)
                                .map_err(LoadError::Windows)?;
                            WaitTarget::Process(
                                image.process.target(h).map_err(LoadError::Windows)?.id,
                            )
                        }
                    } else {
                        WaitTarget::Thread(
                            image
                                .threads
                                .target(h, 0x100000)
                                .map_err(LoadError::Windows)?,
                        )
                    };
                    if all && targets.contains(&target) {
                        return Err(LoadError::Windows(87));
                    }
                    targets.push(target);
                }
                PendingKind::Wait {
                    alertable,
                    targets,
                    all,
                    messages: message,
                    deadline: (timeout != u32::MAX)
                        .then(|| now.saturating_add(u64::from(timeout) * 10000)),
                }
            }
            ApiId::Delay(D::Sleep | D::SleepEx) => {
                let timeout = a[0] as u32;
                PendingKind::Sleep {
                    alertable: id == ApiId::Delay(D::SleepEx) && a[1] != 0,
                    deadline: (timeout != u32::MAX)
                        .then(|| now.saturating_add(u64::from(timeout) * 10000)),
                }
            }
            _ => unreachable!(),
        }
    };
    let signaling = id == ApiId::Wait(V::SignalAndWait);
    if signaling && !retry {
        image
            .objects
            .signal(a[0], image.threads.active)
            .map_err(LoadError::Windows)?;
        if let PendingKind::Wait { targets, .. } = &kind {
            for target in targets {
                if let WaitTarget::Object(id) = target {
                    image.objects.retain_wait(*id);
                }
            }
        }
        image.set_pending(call.clone(), kind.clone(), state.ip);
    }
    if matches!(
        kind,
        PendingKind::Wait {
            alertable: true,
            ..
        } | PendingKind::Sleep {
            alertable: true,
            ..
        }
    ) && !image.threads.current().apcs.is_empty()
    {
        return Ok(Some(0xc0));
    }
    let value = match &kind {
        PendingKind::FileInput => unreachable!(),
        PendingKind::Wait {
            targets,
            all,
            deadline,
            messages,
            ..
        } => wait_value(
            image,
            image.threads.active,
            targets,
            *all,
            *deadline,
            *messages,
            now,
        )?,
        PendingKind::Sleep { deadline, .. } => deadline.is_some_and(|d| now >= d).then_some(0),
        PendingKind::Message
        | PendingKind::Critical
        | PendingKind::Inline
        | PendingKind::Address(_)
        | PendingKind::Condition(_) => unreachable!(),
    };
    if value.is_none() && !retry && !signaling {
        if let PendingKind::Wait { targets, .. } = &kind {
            for target in targets {
                if let WaitTarget::Object(id) = target {
                    image.objects.retain_wait(*id);
                }
            }
        }
        image.set_pending(call.clone(), kind, state.ip);
    }
    Ok(value)
}

pub(super) fn dispatch(
    contract: &Contract,
    services: &Services,
    state: &mut State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
    sample: u64,
) -> Result<(), ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "thread services require a loaded image",
    ))?;
    let call = if let Some(p) = image.pending().filter(|p| p.pc == state.ip) {
        p.call.clone()
    } else {
        Call::capture(memory.mode(), state, memory, contract)?
    };
    if matches!(
        contract.id,
        ApiId::CriticalSection(_) | ApiId::InlineSync(_) | ApiId::AddressWait(_)
    ) {
        let value = match contract.id {
            ApiId::CriticalSection(id) => {
                critical_sections::dispatch(id, &call, state, memory, image.threads.active)?
            }
            ApiId::InlineSync(id) => {
                inline_sync::dispatch(id, &call, state, memory, image, sample)?
            }
            ApiId::AddressWait(id) => {
                address_waits::dispatch(id, &call, state, memory, image, sample)?
            }
            _ => unreachable!(),
        };
        if let Some(value) = value {
            image.clear_pending();
            call.complete(state, value);
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        if matches!(contract.id, ApiId::CriticalSection(_)) {
            image.set_pending(call, PendingKind::Critical, state.ip);
        }
        return Err(ServiceError::Blocked);
    }
    let result = match contract.id {
        ApiId::ThreadControl(C::QueueApc) => {
            image.queue_apc(&call.arguments, memory, services).map(Some)
        }
        ApiId::ThreadControl(C::Create) => image.create_thread(memory, &call.arguments).map(Some),
        ApiId::ThreadControl(C::Context | C::SetContext) => image
            .thread_context(
                contract.id == ApiId::ThreadControl(C::SetContext),
                &call,
                state,
                memory,
            )
            .map(Some),
        ApiId::ThreadControl(C::Exit) => {
            if !image.can_exit_thread() {
                Err(LoadError::Windows(170))
            } else {
                image.exit_thread(call.arguments[0] as u32, state, memory.mode());
                state.work = state.work.wrapping_add(1);
                return Ok(());
            }
        }
        ApiId::ThreadProperty(id) => property(id, &call.arguments, memory, image).map(Some),
        ApiId::Delay(D::SwitchThread) => {
            let ready = image.can_switch_thread();
            image.threads.yield_requested = ready;
            Ok(Some(u64::from(ready)))
        }
        id => waiting(id, &call, state, memory, image, sample),
    };
    let value = match result {
        Ok(Some(0xc0))
            if matches!(
                contract.id,
                ApiId::Delay(D::SleepEx)
                    | ApiId::Wait(_)
                    | ApiId::Winsock(W::WaitEvents)
                    | ApiId::Message(M::WaitObjectsEx)
            ) =>
        {
            image.deliver_apcs(call, state, memory, services)?;
            state.work = state.work.wrapping_add(1);
            return Ok(());
        }
        Ok(Some(value)) => value,
        Ok(None) => return Err(ServiceError::Blocked),
        Err(LoadError::Fault(fault)) => return Err(ServiceError::Fault(fault)),
        Err(LoadError::Windows(code)) => {
            memory.write_int(
                context::error_address(state, memory.mode()),
                4,
                u64::from(code),
            )?;
            match contract.id {
                ApiId::ThreadProperty(P::Suspend | P::Resume)
                | ApiId::Wait(_)
                | ApiId::Winsock(W::WaitEvents)
                | ApiId::Message(_) => u64::from(u32::MAX),
                ApiId::ThreadProperty(P::GetPriority) => 0x7fffffff,
                _ => 0,
            }
        }
    };
    if matches!(contract.id, ApiId::Delay(D::Sleep | D::SleepEx)) {
        image.threads.yield_requested = true;
    }
    image.clear_pending();
    call.complete(state, value);
    state.work = state.work.wrapping_add(1);
    Ok(())
}
