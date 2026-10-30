//! Guest calendar calls share pure Windows arithmetic and one process zone.
use super::{
    calendar::{calendar_to_ticks, ticks_to_calendar},
    call::{peek_slot, Call},
    catalog::{ApiId, CalendarApi as C},
    context,
    timezone::{self, Profile, Zone},
    ClockSource, ServiceError,
};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
};

pub(super) fn implemented(id: ApiId) -> bool {
    matches!(id, ApiId::Calendar(_))
}

pub(super) fn clock_source(id: C, state: &State, memory: &Memory, zone: &Profile) -> ClockSource {
    match id {
        C::SystemTime | C::LocalTime | C::PreciseFileTime => ClockSource::Wall,
        C::TimeZone | C::FileToLocalFileTime if zone.seasonal() => ClockSource::Wall,
        C::SystemTimes => {
            let mut returned = [0; 8];
            let frame = memory
                .observe(
                    memory.mode().address(state.regs[4]),
                    &mut returned[..memory.mode() as usize / 8],
                )
                .is_ok();
            let outputs = (0..3).all(|i| {
                peek_slot(state, memory, i)
                    .is_some_and(|p| p == 0 || memory.check(p, 8, WRITE).is_ok())
            });
            if frame && outputs {
                ClockSource::Cpu
            } else {
                ClockSource::None
            }
        }
        _ => ClockSource::None,
    }
}

fn fields(memory: &Memory, address: u64) -> Result<[u16; 8], ServiceError> {
    let mut bytes = [0; 16];
    memory.read(address, &mut bytes)?;
    Ok(timezone::fields(&bytes))
}

fn failed(memory: &mut Memory, state: &State, code: u32) -> Result<u64, ServiceError> {
    memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
    Ok(0)
}

pub(super) fn dispatch(
    id: C,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    sample: u64,
    cpu_times: Option<Result<[u64; 3], u32>>,
    zone: &Profile,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    match id {
        C::SystemTime | C::LocalTime => {
            let fields = if id == C::LocalTime {
                zone.local(sample).ok()
            } else {
                ticks_to_calendar(sample)
            }
            .ok_or(ServiceError::Unavailable(
                "wall clock is outside the Windows calendar range",
            ))?;
            memory.write(a[0], &timezone::encode(fields))?;
            Ok(0)
        }
        C::PreciseFileTime => {
            memory.write_int(a[0], 8, sample)?;
            Ok(0)
        }
        C::TimeZone => match zone.information(sample) {
            Ok((mode, bytes)) => {
                memory.write(a[0], &bytes)?;
                Ok(mode)
            }
            Err(code) => {
                failed(memory, state, code)?;
                Ok(u32::MAX.into())
            }
        },
        C::SystemToFileTime => {
            let input = fields(memory, a[0])?;
            let Some(ticks) = calendar_to_ticks(input) else {
                return failed(memory, state, 87);
            };
            memory.write_int(a[1], 8, ticks)?;
            Ok(1)
        }
        C::FileToSystemTime => {
            let ticks = memory.read_int(a[0], 8)?;
            let Some(fields) = ticks_to_calendar(ticks) else {
                return failed(memory, state, 87);
            };
            memory.write(a[1], &timezone::encode(fields))?;
            Ok(1)
        }
        C::CompareFileTime => {
            let left = memory.read_int(a[0], 8)?;
            let right = memory.read_int(a[1], 8)?;
            Ok(match left.cmp(&right) {
                std::cmp::Ordering::Less => u32::MAX.into(),
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        }
        C::FileToLocalFileTime => {
            let ticks = memory.read_int(a[0], 8)?;
            match zone.local_filetime(ticks, sample) {
                Ok(local) => {
                    memory.write_int(a[1], 8, local)?;
                    Ok(1)
                }
                Err(code) => failed(memory, state, code),
            }
        }
        C::SystemToLocalTime | C::LocalToSystemTime => {
            let input = fields(memory, a[1])?;
            let mut bytes = [0; 172];
            if a[0] != 0 {
                memory.read(a[0], &mut bytes)?;
            }
            let converted = if a[0] == 0 {
                zone.convert(input, id == C::LocalToSystemTime)
            } else {
                (|| {
                    let ticks = timezone::ticks(input)?;
                    let zone = Zone::parse(&bytes)?;
                    if id == C::LocalToSystemTime {
                        zone.to_utc(ticks)
                    } else {
                        zone.to_local(ticks)
                    }
                })()
            };
            match converted {
                Ok(fields) => {
                    memory.write(a[2], &timezone::encode(fields))?;
                    Ok(1)
                }
                Err(code) => failed(memory, state, code),
            }
        }
        C::SystemTimes => {
            for &pointer in &a[..3] {
                if pointer != 0 {
                    memory.check(pointer, 8, WRITE)?;
                }
            }
            match cpu_times.unwrap_or(Err(50)) {
                Ok(times) => {
                    if times[1] < times[0] {
                        return Err(ServiceError::Unavailable(
                            "CPU kernel time excludes idle time",
                        ));
                    }
                    for (&pointer, ticks) in a[..3].iter().zip(times) {
                        if pointer != 0 {
                            memory.write_int(pointer, 8, ticks)?;
                        }
                    }
                    Ok(1)
                }
                Err(code) => failed(memory, state, code),
            }
        }
    }
}
