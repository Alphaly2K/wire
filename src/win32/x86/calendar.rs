//! PE32 host calendar copy-out keeps its historical error and cast behavior.
use crate::{mem::GuestMem, win32::catalog::CalendarApi};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn call(
    id: CalendarApi,
    pointer: u32,
    mem: &GuestMem,
    last_error: &mut u32,
) -> (u32, String) {
    if id == CalendarApi::TimeZone {
        return match mem.write_bytes(pointer, &[0; 172]) {
            Ok(()) => (0, "code-page / sandbox directory shim".into()),
            Err(_) => {
                *last_error = 87;
                (0, "Win32 error 87".into())
            }
        };
    }
    let result = (|| -> Result<u32, u32> {
        let local = match id {
            CalendarApi::SystemTime => false,
            CalendarApi::LocalTime => true,
            _ => unreachable!("PE32 calendar conversions keep ABI-only error 120"),
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| 87u32)?;
        let seconds = now.as_secs() as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let result = unsafe {
            if local {
                libc::localtime_r(&seconds, &mut tm)
            } else {
                libc::gmtime_r(&seconds, &mut tm)
            }
        };
        if result.is_null() {
            return Err(87);
        }
        let fields = [
            (tm.tm_year + 1900) as u16,
            (tm.tm_mon + 1) as u16,
            tm.tm_wday as u16,
            tm.tm_mday as u16,
            tm.tm_hour as u16,
            tm.tm_min as u16,
            tm.tm_sec as u16,
            now.subsec_millis() as u16,
        ];
        let bytes: Vec<_> = fields.iter().flat_map(|v| v.to_le_bytes()).collect();
        mem.write_bytes(pointer, &bytes).map_err(|_| 87u32)?;
        Ok(0)
    })();
    match result {
        Ok(value) => (value, "x86 guest hardware / monotonic clock profile".into()),
        Err(error) => {
            *last_error = error;
            (0, format!("Win32 error {error}"))
        }
    }
}
