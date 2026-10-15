//! Waitable timers use monotonic deadlines and the cooperative wait scheduler.
use super::{Memory, Runtime64, SystemTime, UNIX_EPOCH};
use crate::win32::catalog::TimerApi;
use std::time::{Duration, Instant};

impl Runtime64 {
    pub(super) fn timer_call(
        &mut self,
        mem: &mut Memory,
        id: TimerApi,
        a: &[u64],
    ) -> Result<u64, String> {
        let result = if id.creating() || id.opening() {
            let opening = id.opening();
            let extended = id.extended();
            let pointer = a[if extended { 1 } else { 2 }];
            let object_name = if pointer == 0 {
                None
            } else {
                Some(Self::path_name(mem, pointer, id.wide())?)
            };
            if opening {
                object_name
                    .ok_or(87u32)
                    .and_then(|n| self.events.open_timer(n, a[0] as u32))
                    .map(|h| (h, false))
            } else if a[0] != 0 {
                Err(50)
            } else if extended && a[2] & !3 != 0 {
                Err(87)
            } else {
                self.events.timer(
                    if extended { a[2] & 1 != 0 } else { a[1] != 0 },
                    object_name,
                    if extended { a[3] as u32 } else { 0x1f0003 },
                )
            }
        } else if id == TimerApi::Cancel {
            self.events
                .set_timer(a[0], None, Duration::ZERO)
                .map(|()| (1, false))
        } else {
            let due = mem.read_int(a[1], 8)? as i64;
            if (a[2] as i32) < 0 {
                Err(87)
            } else if a[3] != 0 || a[5] != 0 {
                // APC delivery and system-wake contexts are not implemented.
                Err(50)
            } else {
                let ticks = if due < 0 {
                    due.unsigned_abs() as u128
                } else {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default();
                    let filetime = (now.as_secs() as u128 + 11_644_473_600) * 10_000_000
                        + now.subsec_nanos() as u128 / 100;
                    (due as u128).saturating_sub(filetime)
                };
                let delay = Duration::new(
                    (ticks / 10_000_000) as u64,
                    (ticks % 10_000_000 * 100) as u32,
                );
                match Instant::now().checked_add(delay) {
                    None => Err(87),
                    Some(deadline) => self
                        .events
                        .set_timer(a[0], Some(deadline), Duration::from_millis(a[2]))
                        .map(|()| (1, false)),
                }
            }
        };
        match result {
            Ok((value, existed)) => {
                if id.creating() {
                    self.set_error(mem, if existed { 183 } else { 0 })?;
                }
                Ok(value)
            }
            Err(error) => {
                self.set_error(mem, error)?;
                Ok(0)
            }
        }
    }
}
