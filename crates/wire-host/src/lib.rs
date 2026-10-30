//! Native platform values for the browser host; no guest execution.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(dead_code)]
#[path = "../../../src/win32/kernel/calendar.rs"]
mod calendar;
mod cpu_time;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod time_zone;
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[allow(dead_code)]
#[path = "../../../src/win32/kernel/timezone.rs"]
mod timezone;

/// Write aggregate idle, kernel (including idle), and user durations in 100 ns units.
/// Return zero on success or a Windows error code, preserving output on failure.
///
/// # Safety
/// A non-null aligned output must point to three writable `u64` values.
#[no_mangle]
pub unsafe extern "C" fn wire_host_cpu_times(output: *mut u64) -> u32 {
    if output.is_null() || !output.is_aligned() {
        return 87;
    }
    match cpu_time::system_cpu_times() {
        Ok(values) => {
            unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), output, 3) };
            0
        }
        Err(code) => code,
    }
}

/// Write a 172-byte Windows TIME_ZONE_INFORMATION record for the host zone.
/// Return zero on success or a Windows error, preserving output on failure.
///
/// # Safety
/// A non-null output must point to 172 writable bytes.
#[no_mangle]
pub unsafe extern "C" fn wire_host_timezone(output: *mut u8) -> u32 {
    if output.is_null() {
        return 87;
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        match time_zone::host_zone() {
            Ok((_, bytes)) => {
                unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
                0
            }
            Err(code) => code,
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        50
    }
}
