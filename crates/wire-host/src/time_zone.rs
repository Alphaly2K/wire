//! Native host timezone discovery; rule arithmetic belongs to the shared core.
use super::{
    calendar::month_days,
    timezone::{calendar, ticks},
};
const EPOCH: i128 = 116444736000000000;
#[allow(dead_code)]
pub(crate) fn host_conversion(input: [u16; 8], reverse: bool) -> Result<[u16; 8], u32> {
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
pub(crate) fn host_zone() -> Result<(u64, [u8; 172]), u32> {
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
        let days = month_days(year as u32, wall[1] as u32) as u16;
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
