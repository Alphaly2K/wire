//! Aggregate host CPU durations, copied as values for guest-side encoding.
// Kernel time includes idle time, as GetSystemTimes requires.
#[allow(deprecated)] // Public Mach ABI exposed by libc.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn system_cpu_times() -> Result<[u64; 3], u32> {
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if hz <= 0 {
        return Err(50);
    }
    let convert = |ticks: u64| -> Result<u64, u32> {
        u64::try_from(ticks as u128 * 10_000_000 / hz as u128).map_err(|_| 31)
    };
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn mach_port_deallocate(
                task: libc::mach_port_t,
                name: libc::mach_port_t,
            ) -> libc::kern_return_t;
        }
        let mut load = libc::host_cpu_load_info { cpu_ticks: [0; 4] };
        let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
        let status = unsafe {
            let host = libc::mach_host_self();
            let status = libc::host_statistics(
                host,
                libc::HOST_CPU_LOAD_INFO,
                (&mut load as *mut libc::host_cpu_load_info).cast(),
                &mut count,
            );
            mach_port_deallocate(libc::mach_task_self(), host);
            status
        };
        if status != libc::KERN_SUCCESS || count != libc::HOST_CPU_LOAD_INFO_COUNT {
            return Err(31);
        }
        let idle = u64::from(load.cpu_ticks[libc::CPU_STATE_IDLE as usize]);
        let kernel = idle + u64::from(load.cpu_ticks[libc::CPU_STATE_SYSTEM as usize]);
        let user = u64::from(load.cpu_ticks[libc::CPU_STATE_USER as usize])
            + u64::from(load.cpu_ticks[libc::CPU_STATE_NICE as usize]);
        Ok([convert(idle)?, convert(kernel)?, convert(user)?])
    }
    #[cfg(target_os = "linux")]
    {
        let stats = std::fs::read_to_string("/proc/stat").map_err(|_| 31u32)?;
        let mut values = stats.lines().next().ok_or(31u32)?.split_whitespace();
        if values.next() != Some("cpu") {
            return Err(31);
        }
        let ticks: Vec<u64> = values
            .map(|s| s.parse().map_err(|_| 31u32))
            .collect::<Result<_, _>>()?;
        if ticks.len() < 7 {
            return Err(31);
        }
        let idle = ticks[3]
            .checked_add(*ticks.get(4).unwrap_or(&0))
            .ok_or(31u32)?;
        let kernel = ticks[2]
            .checked_add(idle)
            .and_then(|v| v.checked_add(ticks[5]))
            .and_then(|v| v.checked_add(ticks[6]))
            .ok_or(31u32)?;
        let user = ticks[0].checked_add(ticks[1]).ok_or(31u32)?;
        Ok([convert(idle)?, convert(kernel)?, convert(user)?])
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn system_cpu_times() -> Result<[u64; 3], u32> {
    Err(50)
}
