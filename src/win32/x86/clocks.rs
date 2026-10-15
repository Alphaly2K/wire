//! Clock adapters borrow only the process epoch and checked guest memory.
use crate::{api::Args, mem::GuestMem, win32::catalog::ClockApi};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(crate) enum ClockReturn {
    Dword(u32),
    Qword(u64),
}

pub(crate) fn call(
    id: ClockApi,
    args: &Args,
    mem: &GuestMem,
    start: &Instant,
) -> Result<ClockReturn, String> {
    use ClockApi::*;
    let value = match id {
        TickCount | MultimediaTick => start.elapsed().as_millis() as u32,
        TickCount64 => return Ok(ClockReturn::Qword(start.elapsed().as_millis() as u64)),
        PerformanceCounter => {
            let value = (start.elapsed().as_nanos() / 100) as u64;
            mem.write_u64(args[0], value).map_err(|e| e.to_string())?;
            1
        }
        PerformanceFrequency => {
            mem.write_u64(args[0], 10_000_000)
                .map_err(|e| e.to_string())?;
            1
        }
        SystemFileTime => {
            // Preserve PE32's existing cast-before-division and pre-epoch fallback.
            let unix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            mem.write_u64(args[0], unix / 100 + 116_444_736_000_000_000)
                .map_err(|e| e.to_string())?;
            0
        }
    };
    Ok(ClockReturn::Dword(value))
}
