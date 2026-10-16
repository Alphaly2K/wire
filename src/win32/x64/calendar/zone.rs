//! Checked timezone conversions with owned Windows output buffers.
use crate::{
    win32::kernel::calendar::{calendar_to_ticks, ticks_to_calendar},
    x64::memory::Memory,
};
const EPOCH: i128 = 116444736000000000;
const MINUTE: i128 = 600000000;
fn fields(bytes: &[u8]) -> [u16; 8] {
    std::array::from_fn(|n| u16::from_le_bytes(bytes[n * 2..n * 2 + 2].try_into().unwrap()))
}
fn ticks(fields: [u16; 8]) -> Result<u64, u32> {
    calendar_to_ticks(fields).ok_or(87)
}
fn calendar(ticks: i128) -> Result<[u16; 8], u32> {
    u64::try_from(ticks)
        .ok()
        .and_then(ticks_to_calendar)
        .ok_or(87)
}
struct Zone {
    standard: i64,
    daylight: i64,
    standard_date: [u16; 8],
    daylight_date: [u16; 8],
}
impl Zone {
    fn parse(bytes: &[u8; 172]) -> Result<Self, u32> {
        let bias = |at| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as i64;
        let standard_date = fields(&bytes[68..84]);
        let daylight_date = fields(&bytes[152..168]);
        if (standard_date[1] == 0) != (daylight_date[1] == 0) {
            return Err(87);
        }
        for date in [standard_date, daylight_date] {
            if date[1] != 0
                && (date[1] > 12
                    || date[2] > 6
                    || date[3] == 0
                    || date[0] == 0 && date[3] > 5
                    || date[4] > 23
                    || date[5] > 59
                    || date[6] > 59
                    || date[7] > 999)
            {
                return Err(87);
            }
        }
        Ok(Self {
            // Without transition dates only Bias applies; Windows ignores both extra biases.
            standard: bias(0) + if standard_date[1] == 0 { 0 } else { bias(84) },
            daylight: bias(0) + bias(168),
            standard_date,
            daylight_date,
        })
    }
    fn transition(date: [u16; 8], year: u16) -> Result<u64, u32> {
        let mut date = date;
        if date[0] == 0 {
            date[0] = year;
            let occurrence = date[3];
            date[3] = 1;
            let first = calendar(ticks(date)? as i128)?[2];
            date[3] = 1 + (date[2] + 7 - first) % 7 + (occurrence - 1) * 7;
            if occurrence == 5 && calendar_to_ticks(date).is_none() {
                date[3] -= 7;
            }
        }
        ticks(date)
    }
    fn bias_at(&self, utc: u64) -> Result<i64, u32> {
        if self.standard_date[1] == 0 {
            return Ok(self.standard);
        }
        let year = calendar(utc as i128 - self.standard as i128 * MINUTE)?[0];
        let daylight =
            Self::transition(self.daylight_date, year)? as i128 + self.standard as i128 * MINUTE;
        let standard =
            Self::transition(self.standard_date, year)? as i128 + self.daylight as i128 * MINUTE;
        let utc = utc as i128;
        let summer = if daylight < standard {
            utc >= daylight && utc < standard
        } else {
            utc >= daylight || utc < standard
        };
        Ok(if summer { self.daylight } else { self.standard })
    }
    fn to_local(&self, utc: u64) -> Result<[u16; 8], u32> {
        calendar(utc as i128 - self.bias_at(utc)? as i128 * MINUTE)
    }
    fn to_utc(&self, local: u64) -> Result<[u16; 8], u32> {
        // Resolve an overlap to standard time. A spring-forward gap has no UTC match.
        for bias in [self.standard, self.daylight] {
            let candidate = local as i128 + bias as i128 * MINUTE;
            if let Ok(utc) = u64::try_from(candidate) {
                if self.bias_at(utc)? == bias {
                    return calendar(candidate);
                }
            }
        }
        Err(87)
    }
}
fn host_conversion(input: [u16; 8], reverse: bool) -> Result<[u16; 8], u32> {
    let source = ticks(input)?;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let seconds: libc::time_t = if reverse {
        tm.tm_year = input[0] as i32 - 1900;
        tm.tm_mon = input[1] as i32 - 1;
        tm.tm_mday = input[3] as i32;
        tm.tm_hour = input[4] as i32;
        tm.tm_min = input[5] as i32;
        tm.tm_sec = input[6] as i32;
        tm.tm_isdst = -1;
        // mktime also normalizes nonexistent local times; reject that normalization.
        let seconds = unsafe { libc::mktime(&mut tm) };
        if tm.tm_year != input[0] as i32 - 1900
            || tm.tm_mon != input[1] as i32 - 1
            || tm.tm_mday != input[3] as i32
            || tm.tm_hour != input[4] as i32
            || tm.tm_min != input[5] as i32
            || tm.tm_sec != input[6] as i32
        {
            return Err(87);
        }
        seconds
    } else {
        ((source as i128 - EPOCH).div_euclid(10000000))
            .try_into()
            .map_err(|_| 87u32)?
    };
    let result = unsafe {
        if reverse {
            libc::gmtime_r(&seconds, &mut tm)
        } else {
            libc::localtime_r(&seconds, &mut tm)
        }
    };
    if result.is_null() {
        return Err(87);
    }
    let raw = [
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_wday,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        input[7] as i32,
    ];
    let mut output = [0; 8];
    for (out, value) in output.iter_mut().zip(raw) {
        *out = value.try_into().map_err(|_| 87u32)?;
    }
    ticks(output)?;
    Ok(output)
}
pub(super) fn host_zone() -> Result<(u64, [u8; 172]), u32> {
    fn host_tm(seconds: i64) -> Result<libc::tm, u32> {
        // time_t can be narrower on a 32-bit host.
        #[allow(clippy::useless_conversion)]
        let seconds: libc::time_t = seconds.try_into().map_err(|_| 87u32)?;
        let mut tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::localtime_r(&seconds, &mut tm) }.is_null() {
            return Err(87);
        }
        Ok(tm)
    }
    fn name(tm: &libc::tm) -> Result<Vec<u8>, u32> {
        if tm.tm_zone.is_null() {
            return Err(50);
        }
        // libc owns this host string. Only its copied UTF-16 value enters guest memory.
        let name = unsafe { std::ffi::CStr::from_ptr(tm.tm_zone) }
            .to_str()
            .map_err(|_| 50u32)?;
        let units: Vec<_> = name.encode_utf16().collect();
        if units.len() >= 32 {
            return Err(50);
        }
        Ok(units.into_iter().flat_map(u16::to_le_bytes).collect())
    }
    #[allow(clippy::unnecessary_cast)] // tm_gmtoff is a host-width c_long.
    fn offset(tm: &libc::tm) -> i64 {
        tm.tm_gmtoff as i64
    }
    fn key(tm: &libc::tm) -> (i64, bool) {
        (offset(tm), tm.tm_isdst > 0)
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| 87u32)?
        .as_secs() as i64;
    let current = host_tm(now)?;
    let year = u16::try_from(current.tm_year + 1900).map_err(|_| 87u32)?;
    let start = (ticks([year, 1, 0, 1, 0, 0, 0, 0])? as i128 - EPOCH) / 10000000 - 172800;
    let end = (ticks([year.checked_add(1).ok_or(87u32)?, 1, 0, 1, 0, 0, 0, 0])? as i128 - EPOCH)
        / 10000000
        + 172800;
    let mut previous = host_tm(start as i64)?;
    let mut standard = None;
    let mut daylight = None;
    let mut transitions = Vec::new();
    let mut at = start as i64;
    while at < end as i64 {
        let next = (at + 86400).min(end as i64);
        let sample = host_tm(next)?;
        if key(&previous) != key(&sample) {
            let mut low = at;
            let mut high = next;
            while high - low > 1 {
                let mid = low + (high - low) / 2;
                if key(&host_tm(mid)?) == key(&previous) {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            let after = host_tm(high)?;
            let wall = calendar(EPOCH + (high as i128 + offset(&previous) as i128) * 10000000)?;
            if wall[0] == year {
                // TIME_ZONE_INFORMATION cannot represent political base-offset changes
                // or multiple independent DST periods; report those rather than invent rules.
                if previous.tm_isdst < 0
                    || after.tm_isdst < 0
                    || (previous.tm_isdst > 0) == (after.tm_isdst > 0)
                {
                    return Err(50);
                }
                transitions.push((after.tm_isdst > 0, wall));
            }
        }
        if sample.tm_year + 1900 == year as i32 {
            let slot = if sample.tm_isdst > 0 {
                &mut daylight
            } else {
                &mut standard
            };
            if slot.as_ref().is_some_and(|tm| key(tm) != key(&sample)) {
                return Err(50);
            }
            *slot = Some(sample);
        }
        previous = sample;
        at = next;
    }
    let standard = standard.ok_or(50u32)?;
    if offset(&standard) % 60 != 0 {
        return Err(50);
    }
    let mut data = [0; 172];
    let bias = i32::try_from(-offset(&standard) / 60).map_err(|_| 87u32)?;
    data[..4].copy_from_slice(&bias.to_le_bytes());
    let standard_name = name(&standard)?;
    data[4..4 + standard_name.len()].copy_from_slice(&standard_name);
    let Some(daylight) = daylight else {
        if !transitions.is_empty() {
            return Err(50);
        }
        return Ok((0, data));
    };
    if offset(&daylight) % 60 != 0
        || transitions.len() != 2
        || transitions.iter().filter(|(dst, _)| *dst).count() != 1
    {
        return Err(50);
    }
    let daylight_name = name(&daylight)?;
    data[88..88 + daylight_name.len()].copy_from_slice(&daylight_name);
    let daylight_bias =
        i32::try_from((offset(&standard) - offset(&daylight)) / 60).map_err(|_| 87u32)?;
    data[168..172].copy_from_slice(&daylight_bias.to_le_bytes());
    for (dst, mut wall) in transitions {
        let days = crate::win32::kernel::calendar::month_days(year as u32, wall[1] as u32) as u16;
        wall[0] = 0;
        wall[3] = if wall[3] + 7 > days {
            5
        } else {
            (wall[3] - 1) / 7 + 1
        };
        let offset = if dst { 152 } else { 68 };
        for (n, value) in wall.into_iter().enumerate() {
            data[offset + n * 2..offset + n * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok((if current.tm_isdst > 0 { 2 } else { 1 }, data))
}
pub(super) fn convert_zone(
    mem: &Memory,
    args: &[u64],
    reverse: bool,
) -> Result<Result<[u8; 16], u32>, String> {
    let mut input = [0; 16];
    mem.read(args[1], &mut input)?;
    let mut zone = [0; 172];
    if args[0] != 0 {
        mem.read(args[0], &mut zone)?;
    }
    let converted = (|| {
        let input = fields(&input);
        let value = ticks(input)?;
        if args[0] == 0 {
            host_conversion(input, reverse)
        } else {
            let zone = Zone::parse(&zone)?;
            if reverse {
                zone.to_utc(value)
            } else {
                zone.to_local(value)
            }
        }
    })();
    Ok(converted.map(|fields| {
        let mut bytes = [0; 16];
        for (n, field) in fields.into_iter().enumerate() {
            bytes[n * 2..n * 2 + 2].copy_from_slice(&field.to_le_bytes());
        }
        bytes
    }))
}
