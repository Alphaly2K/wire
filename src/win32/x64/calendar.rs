//! Win64 calendar calls borrow checked memory and the active thread error slot.
use crate::{
    win32::{
        catalog::CalendarApi,
        kernel::calendar::{calendar_to_ticks, ticks_to_calendar},
    },
    x64::memory::{Memory, WRITE},
};
use std::time::{SystemTime, UNIX_EPOCH};
mod zone;

pub(crate) fn call(
    mem: &mut Memory,
    id: CalendarApi,
    args: &[u64],
    last_error: u64,
) -> Result<u64, String> {
    match id {
        CalendarApi::SystemTime | CalendarApi::LocalTime => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?;
            let instant: libc::time_t = now
                .as_secs()
                .try_into()
                .map_err(|_| "host time out of range")?;
            let mut calendar: libc::tm = unsafe { std::mem::zeroed() };
            unsafe {
                let result = if id == CalendarApi::LocalTime {
                    libc::localtime_r(&instant, &mut calendar)
                } else {
                    libc::gmtime_r(&instant, &mut calendar)
                };
                if result.is_null() {
                    return Err("host calendar conversion failed".into());
                }
            };
            let fields = [
                calendar.tm_year + 1900,
                calendar.tm_mon + 1,
                calendar.tm_wday,
                calendar.tm_mday,
                calendar.tm_hour,
                calendar.tm_min,
                calendar.tm_sec,
                now.subsec_millis() as i32,
            ];
            let mut output = [0; 16];
            for (n, value) in fields.into_iter().enumerate() {
                let value =
                    u16::try_from(value).map_err(|_| "Win64 calendar value out of range")?;
                output[n * 2..n * 2 + 2].copy_from_slice(&value.to_le_bytes());
            }
            mem.write(args[0], &output)?;
            Ok(0)
        }
        CalendarApi::TimeZone => match zone::host_zone() {
            Ok((mode, data)) => {
                mem.write(args[0], &data)?;
                Ok(mode)
            }
            Err(error) => {
                mem.write_int(last_error, 4, error as u64)?;
                Ok(u32::MAX as u64)
            }
        },
        CalendarApi::SystemToLocalTime | CalendarApi::LocalToSystemTime => {
            match zone::convert_zone(mem, args, id == CalendarApi::LocalToSystemTime)? {
                Ok(output) => {
                    mem.write(args[2], &output)?;
                    Ok(1)
                }
                Err(error) => {
                    mem.write_int(last_error, 4, error as u64)?;
                    Ok(0)
                }
            }
        }
        CalendarApi::SystemToFileTime => {
            let mut bytes = [0; 16];
            mem.read(args[0], &mut bytes)?;
            let fields =
                std::array::from_fn(|n| u16::from_le_bytes([bytes[n * 2], bytes[n * 2 + 1]]));
            if let Some(ticks) = calendar_to_ticks(fields) {
                mem.write_int(args[1], 8, ticks)?;
                Ok(1)
            } else {
                mem.write_int(last_error, 4, 87)?;
                Ok(0)
            }
        }
        CalendarApi::FileToSystemTime => {
            let ticks = mem.read_int(args[0], 8)?;
            if let Some(fields) = ticks_to_calendar(ticks) {
                let bytes: Vec<u8> = fields.into_iter().flat_map(u16::to_le_bytes).collect();
                mem.write(args[1], &bytes)?;
                Ok(1)
            } else {
                mem.write_int(last_error, 4, 87)?;
                Ok(0)
            }
        }
        CalendarApi::CompareFileTime => {
            use std::cmp::Ordering;
            Ok(
                match mem.read_int(args[0], 8)?.cmp(&mem.read_int(args[1], 8)?) {
                    Ordering::Less => u32::MAX as u64,
                    Ordering::Equal => 0,
                    Ordering::Greater => 1,
                },
            )
        }
        CalendarApi::PreciseFileTime => {
            let elapsed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?;
            let time = 116444736000000000u64 + (elapsed.as_nanos() / 100) as u64;
            mem.write_int(args[0], 8, time)?;
            Ok(0)
        }
        CalendarApi::SystemTimes => {
            for &pointer in &args[..3] {
                if pointer != 0 {
                    mem.check(pointer, 8, WRITE)?;
                }
            }
            match crate::host::cpu_time::system_cpu_times() {
                Ok(times) => {
                    for (&pointer, ticks) in args[..3].iter().zip(times) {
                        if pointer != 0 {
                            mem.write_int(pointer, 8, ticks)?;
                        }
                    }
                    Ok(1)
                }
                Err(error) => {
                    mem.write_int(last_error, 4, error as u64)?;
                    Ok(0)
                }
            }
        }
        CalendarApi::FileToLocalFileTime => {
            unreachable!("FileTimeToLocalFileTime has no Win64 binding")
        }
    }
}
