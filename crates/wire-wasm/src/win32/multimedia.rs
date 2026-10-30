use super::{call::Call, catalog::MultimediaApi as M, period_requests, ServiceError};
use crate::{machine::Mode, memory::Memory, process::Loaded};
pub(super) fn dispatch(
    id: M,
    call: &Call,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let a = call.arguments;
    if id == M::SetEvent {
        return Ok(0);
    }
    if id == M::KillEvent {
        return Ok(97);
    }
    if id == M::Caps {
        if a[1] < 8 || a[0] == 0 {
            return Ok(97);
        }
        let result = memory.write(a[0], &period_requests::CAPS);
        return if memory.mode() == Mode::X86 {
            Ok(if result.is_ok() { 0 } else { 97 })
        } else {
            result.map(|_| 0).map_err(Into::into)
        };
    }
    if memory.mode() == Mode::X86 {
        return Ok(if a[0] == 0 { 97 } else { 0 });
    }
    let periods = &mut image
        .ok_or(ServiceError::Unavailable(
            "timer requests require a loaded image",
        ))?
        .periods;
    match id {
        M::BeginPeriod => periods
            .begin(a[0] as u32)
            .map_err(|_| ServiceError::Unavailable("multimedia timer reference capacity")),
        M::EndPeriod => Ok(periods.end(a[0] as u32)),
        M::Caps | M::SetEvent | M::KillEvent => unreachable!(),
    }
}
