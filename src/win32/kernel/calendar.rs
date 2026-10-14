//! Pure Windows SYSTEMTIME/FILETIME calendar arithmetic.
fn days_before_year(year: u32) -> u64 {
    let y = year as u64 - 1;
    y * 365 + y / 4 - y / 100 + y / 400
}
pub(crate) fn month_days(year: u32, month: u32) -> u32 {
    match month {
        2 => {
            28 + (year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)))
                as u32
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
pub(crate) fn calendar_to_ticks(fields: [u16; 8]) -> Option<u64> {
    let [year, month, _, day, hour, minute, second, millis] = fields.map(u32::from);
    if !(1601..=30827).contains(&year)
        || !(1..=12).contains(&month)
        || day == 0
        || day > month_days(year, month)
        || hour > 23
        || minute > 59
        || second > 59
        || millis > 999
    {
        return None;
    }
    let days = days_before_year(year) - days_before_year(1601)
        + (1..month).map(|m| month_days(year, m) as u64).sum::<u64>()
        + day as u64
        - 1;
    Some(
        (days * 86400 + hour as u64 * 3600 + minute as u64 * 60 + second as u64) * 10_000_000
            + millis as u64 * 10000,
    )
}
pub(crate) fn ticks_to_calendar(ticks: u64) -> Option<[u16; 8]> {
    let day = ticks / 864000000000;
    let origin = days_before_year(1601);
    if ticks >= 1 << 63 || day >= days_before_year(30828) - origin {
        return None;
    }
    let (mut low, mut high) = (1601, 30828);
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if days_before_year(mid) - origin <= day {
            low = mid;
        } else {
            high = mid;
        }
    }
    let mut remaining = day - (days_before_year(low) - origin);
    let mut month = 1;
    while remaining >= month_days(low, month) as u64 {
        remaining -= month_days(low, month) as u64;
        month += 1;
    }
    let seconds = (ticks / 10_000_000) % 86400;
    Some([
        low as u16,
        month as u16,
        ((day + 1) % 7) as u16,
        remaining as u16 + 1,
        (seconds / 3600) as u16,
        ((seconds / 60) % 60) as u16,
        (seconds % 60) as u16,
        ((ticks / 10000) % 1000) as u16,
    ])
}
