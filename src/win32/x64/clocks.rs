//! General clock calls retain the same values as the dedicated hot routes.
use crate::{win32::catalog::ClockApi, x64::memory::Memory};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(crate) fn call(
    id: ClockApi,
    args: &[u64],
    mem: &mut Memory,
    start: &Instant,
) -> Result<u64, String> {
    use ClockApi::*;
    Ok(match id {
        TickCount | MultimediaTick => start.elapsed().as_millis() as u32 as u64,
        TickCount64 => start.elapsed().as_millis() as u64,
        PerformanceCounter => {
            mem.write_int(args[0], 8, (start.elapsed().as_nanos() / 100) as u64)?;
            1
        }
        PerformanceFrequency => {
            mem.write_int(args[0], 8, 10_000_000)?;
            1
        }
        SystemFileTime => {
            let elapsed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?;
            let time = 116444736000000000u64 + (elapsed.as_nanos() / 100) as u64;
            mem.write_int(args[0], 8, time)?;
            0
        }
    })
}
