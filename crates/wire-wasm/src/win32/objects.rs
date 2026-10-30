use super::{
    call::{peek_slot, Call},
    catalog::{ApiId, MutexApi as M, SyncObjectApi as S, TimerApi as T},
    context, files,
    modules::{self, Failure},
    object_state, ClockSource, ServiceError,
};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};
use std::time::Duration;

#[derive(Default)]
pub(crate) struct Ticks(pub u64);
impl object_state::Clock for Ticks {
    type Instant = u64;
    fn now(&self) -> u64 {
        self.0
    }
    fn next_period(&self, deadline: u64, now: u64, period: Duration) -> Option<u64> {
        let period = u64::try_from(period.as_nanos() / 100).ok()?.max(1);
        now.checked_add(period - (now - deadline) % period)
    }
}
pub(crate) type Objects = object_state::Objects<Ticks>;
pub(crate) fn new() -> Objects {
    Objects::new(Ticks::default(), 0x9000_0000, super::tokens::START_X86)
}
pub(super) fn implemented(id: ApiId) -> bool {
    matches!(id, ApiId::SyncObject(_)) || matches!(id, ApiId::Mutex(_) | ApiId::Timer(_))
}
pub(super) fn clock_source(
    id: ApiId,
    state: &State,
    memory: &Memory,
    image: Option<&Loaded>,
) -> ClockSource {
    match id {
        ApiId::SyncObject(S::PulseEvent)
            if image
                .is_some_and(|i| peek_slot(state, memory, 0).is_some_and(|h| i.pulse_clock(h))) =>
        {
            ClockSource::Elapsed
        }
        ApiId::Timer(T::Set | T::SetEx) => {
            let due = peek_slot(state, memory, 1).and_then(|address| {
                let mut bytes = [0; 8];
                memory.observe(address, &mut bytes).ok()?;
                Some(i64::from_le_bytes(bytes))
            });
            match due {
                Some(d) if d < 0 => ClockSource::Elapsed,
                Some(_) => ClockSource::ElapsedWall,
                None => ClockSource::None,
            }
        }
        ApiId::Timer(T::Cancel)
            if image.is_some_and(|i| {
                peek_slot(state, memory, 0).is_some_and(|h| i.objects.handle_needs_time(h))
            }) =>
        {
            ClockSource::Elapsed
        }
        _ => ClockSource::None,
    }
}
fn object_name(memory: &Memory, pointer: u64, wide: bool) -> Result<Option<String>, Failure> {
    if pointer == 0 {
        Ok(None)
    } else {
        modules::name(memory, pointer, wide).map(Some)
    }
}
fn sync(
    id: S,
    a: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
    error: u64,
    sample: u64,
) -> Result<u64, Failure> {
    if id == S::PulseEvent {
        return image.pulse_event(a[0], sample).map_err(Failure::Windows);
    }
    if id == S::SetEvent || id == S::ResetEvent {
        return image
            .objects
            .change(a[0], id == S::SetEvent)
            .map_err(Failure::Windows);
    }
    if id == S::ReleaseSemaphore {
        let previous = image
            .objects
            .release_count(a[0], a[1] as i32)
            .map_err(Failure::Windows)?;
        if a[2] != 0 {
            memory.write_int(a[2], 4, previous as u32 as u64)?;
        }
        image.objects.release(a[0], a[1] as i32);
        return Ok(1);
    }
    let opening = id.opening();
    let extended = id.extended();
    let event = matches!(
        id,
        S::CreateEventA
            | S::CreateEventW
            | S::CreateEventExA
            | S::CreateEventExW
            | S::OpenEventA
            | S::OpenEventW
    );
    let inherit = if opening {
        a[1] != 0
    } else {
        files::security(memory, a[0])?
    };
    let pointer = a[if opening {
        2
    } else if event && extended {
        1
    } else {
        3
    }];
    let name = object_name(memory, pointer, id.wide())?;
    if !opening {
        memory.check(error, 4, WRITE)?;
    }
    let result = if opening {
        image
            .objects
            .open(name.ok_or(Failure::Windows(87))?, a[0] as u32, !event)
            .map(|h| (h, false))
    } else if event {
        if extended && a[2] & !3 != 0 {
            return Err(Failure::Windows(87));
        }
        image.objects.create(
            if extended { a[2] & 1 != 0 } else { a[1] != 0 },
            if extended { a[2] & 2 != 0 } else { a[2] != 0 },
            name,
            if extended { a[3] as u32 } else { 0x1f0003 },
        )
    } else {
        if extended && a[4] != 0 {
            return Err(Failure::Windows(87));
        }
        image.objects.semaphore(
            a[1] as i32,
            a[2] as i32,
            name,
            if extended { a[5] as u32 } else { 0x1f0003 },
        )
    };
    let (handle, existed) = result.map_err(Failure::Windows)?;
    if inherit {
        image
            .objects
            .set_flags(handle, 1, 1)
            .expect("new object handle");
    }
    if !opening {
        memory.write_int(error, 4, if existed { 183 } else { 0 })?;
    }
    Ok(handle)
}
fn mutex(
    id: M,
    a: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
    error: u64,
) -> Result<u64, Failure> {
    if id == M::Release {
        image
            .objects
            .release_mutex(a[0], image.threads.active)
            .map_err(Failure::Windows)?;
        return Ok(1);
    }
    let opening = matches!(id, M::OpenA | M::OpenW);
    let extended = matches!(id, M::CreateExA | M::CreateExW);
    let wide = matches!(id, M::CreateW | M::CreateExW | M::OpenW);
    let inherit = if opening {
        a[1] != 0
    } else {
        files::security(memory, a[0])?
    };
    let name = object_name(memory, a[if extended { 1 } else { 2 }], wide)?;
    if !opening {
        memory.check(error, 4, WRITE)?;
    }
    let result = if opening {
        image
            .objects
            .open_mutex(name.ok_or(Failure::Windows(87))?, a[0] as u32)
            .map(|h| (h, false))
    } else {
        if extended && a[2] & !1 != 0 {
            return Err(Failure::Windows(87));
        }
        let own = if extended { a[2] & 1 != 0 } else { a[1] != 0 };
        image.objects.mutex(
            own.then_some(image.threads.active),
            name,
            if extended { a[3] as u32 } else { 0x1f0001 },
        )
    };
    let (handle, existed) = result.map_err(Failure::Windows)?;
    if inherit {
        image
            .objects
            .set_flags(handle, 1, 1)
            .expect("new mutex handle");
    }
    if !opening {
        memory.write_int(error, 4, if existed { 183 } else { 0 })?;
    }
    Ok(handle)
}
fn timer(
    id: T,
    a: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
    error: u64,
    sample: u64,
    wall: Option<u64>,
) -> Result<u64, ServiceError> {
    let result = (|| -> Result<u64, Failure> {
        if id.creating() || id.opening() {
            let opening = id.opening();
            let extended = id.extended();
            let inherit = if opening {
                a[1] != 0
            } else {
                files::security(memory, a[0])?
            };
            let name = object_name(memory, a[if extended { 1 } else { 2 }], id.wide())?;
            if id.creating() {
                memory.check(error, 4, WRITE)?;
            }
            let result = if opening {
                image
                    .objects
                    .open_timer(name.ok_or(Failure::Windows(87))?, a[0] as u32)
                    .map(|h| (h, false))
            } else {
                if extended && a[2] & !3 != 0 {
                    return Err(Failure::Windows(87));
                }
                image.objects.timer(
                    if extended { a[2] & 1 != 0 } else { a[1] != 0 },
                    name,
                    if extended { a[3] as u32 } else { 0x1f0003 },
                )
            };
            let (handle, existed) = result.map_err(Failure::Windows)?;
            if inherit {
                image
                    .objects
                    .set_flags(handle, 1, 1)
                    .expect("new timer handle");
            }
            if id.creating() {
                memory.write_int(error, 4, if existed { 183 } else { 0 })?;
            }
            return Ok(handle);
        }
        image.objects.clock.0 = image.objects.clock.0.max(sample);
        if id == T::Cancel {
            image
                .objects
                .set_timer(a[0], None, Duration::ZERO)
                .map_err(Failure::Windows)?;
            return Ok(1);
        }
        let due = memory.read_int(a[1], 8)? as i64;
        if (a[2] as i32) < 0 {
            return Err(Failure::Windows(87));
        }
        if a[3] != 0 || a[5] != 0 {
            return Err(Failure::Windows(50));
        }
        let ticks = if due < 0 {
            due.unsigned_abs()
        } else {
            (due as u64).saturating_sub(wall.unwrap_or(0))
        };
        let deadline = image
            .objects
            .clock
            .0
            .checked_add(ticks)
            .ok_or(Failure::Windows(87))?;
        image
            .objects
            .set_timer(
                a[0],
                Some(deadline),
                Duration::from_millis(a[2] as u32 as u64),
            )
            .map_err(Failure::Windows)?;
        Ok(1)
    })();
    finish(result, memory, error)
}
fn finish(
    result: Result<u64, Failure>,
    memory: &mut Memory,
    error: u64,
) -> Result<u64, ServiceError> {
    match result {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(ServiceError::Fault(fault)),
        Err(Failure::Windows(code)) => {
            memory.write_int(error, 4, u64::from(code))?;
            Ok(0)
        }
    }
}
pub(super) fn dispatch(
    id: ApiId,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
    sample: u64,
    wall: Option<u64>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "synchronization services require a loaded image",
    ))?;
    let error = context::error_address(state, memory.mode());
    match id {
        ApiId::SyncObject(id) => {
            let result = sync(id, &call.arguments, memory, image, error, sample);
            finish(result, memory, error)
        }
        ApiId::Mutex(id) => {
            let result = mutex(id, &call.arguments, memory, image, error);
            finish(result, memory, error)
        }
        ApiId::Timer(id) => {
            if matches!(id, T::Set | T::SetEx) && wall.is_none() {
                let mut bytes = [0; 8];
                if memory.observe(call.arguments[1], &mut bytes).is_ok()
                    && i64::from_le_bytes(bytes) >= 0
                {
                    return Err(ServiceError::Unavailable(
                        "absolute timer requires elapsed and FILETIME samples",
                    ));
                }
            }
            timer(id, &call.arguments, memory, image, error, sample, wall)
        }
        _ => unreachable!(),
    }
}
