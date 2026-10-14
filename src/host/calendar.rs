//! Host local calendar provider; Windows output encoding belongs to adapters.
pub(crate) fn local_system_time() -> Result<[u16; 8], String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    let instant = now.as_secs() as libc::time_t;
    let mut calendar: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&instant, &mut calendar) }.is_null() {
        return Err("host local calendar unavailable".into());
    }
    Ok([
        (calendar.tm_year + 1900) as u16,
        (calendar.tm_mon + 1) as u16,
        calendar.tm_wday as u16,
        calendar.tm_mday as u16,
        calendar.tm_hour as u16,
        calendar.tm_min as u16,
        calendar.tm_sec as u16,
        now.subsec_millis() as u16,
    ])
}
