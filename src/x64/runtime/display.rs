//! Virtual desktop queries and explicit process-owned monitor callbacks.
use super::{threads::Threads, user::User, Cpu64, Memory, Runtime64, READ, WRITE};
pub(in crate::x64::runtime) use crate::win32::user::display::{DEVICE, HEIGHT, REFRESH_HZ, WIDTH};
use crate::win32::x64::geometry::read_rect as rect;
use crate::win32::{catalog::DisplayApi, user::display as profile};
pub(in crate::x64::runtime) const MONITOR: u64 = super::TEB + 0x600008;
pub(super) struct Enumeration {
    pub result: u64,
    pub calls: Vec<(u64, [u64; 4])>,
    pub release: Vec<u64>,
}
impl Runtime64 {
    pub(super) fn display_call(
        &self,
        mem: &mut Memory,
        id: DisplayApi,
        args: &[u64],
    ) -> Result<u64, String> {
        self.user.display_call(mem, id, args, &self.threads)
    }
}

impl User {
    pub(super) fn display_call(
        &self,
        mem: &mut Memory,
        id: DisplayApi,
        args: &[u64],
        threads: &Threads,
    ) -> Result<u64, String> {
        let wide = id.wide();
        if id.devices() || id.settings() {
            let device = if args[0] == 0 {
                String::new()
            } else {
                crate::win32::x64::text::path_name(mem, args[0], wide)?
            };
            if id.devices() {
                let size = if wide { 840 } else { 424 };
                if mem.read_int(args[2], 4)? != size || args[3] & !1 != 0 {
                    threads.set_error(mem, 87)?;
                    return Ok(0);
                }
                if args[1] != 0 || (!device.is_empty() && !device.eq_ignore_ascii_case(DEVICE)) {
                    return Ok(0);
                }
                let adapter = device.is_empty();
                let bytes = profile::device(
                    wide,
                    adapter,
                    if adapter {
                        "Wire virtual display"
                    } else {
                        "Wire virtual monitor"
                    },
                );
                mem.write(args[2], &bytes)?;
                return Ok(1);
            }
            if (!device.is_empty() && !device.eq_ignore_ascii_case(DEVICE))
                || !matches!(args[1], 0 | 0xffffffff | 0xfffffffe)
            {
                return Ok(0);
            }
            if id.extended() && args[3] & !3 != 0 {
                threads.set_error(mem, 87)?;
                return Ok(0);
            }
            let size_offset = if wide { 68 } else { 36 };
            let size = mem.read_int(
                args[2].checked_add(size_offset).ok_or("DEVMODE overflow")?,
                2,
            )? as usize;
            if !(if wide { 188..=220 } else { 124..=156 }).contains(&size) {
                threads.set_error(mem, 87)?;
                return Ok(0);
            }
            mem.write(args[2], &profile::mode(wide, size))?;
            return Ok(1);
        }
        match id {
            DisplayApi::Metrics => Ok(profile::metric(args[0] as u32) as u64),
            DisplayApi::InfoA | DisplayApi::InfoW => {
                let size = mem.read_int(args[1], 4)? as usize;
                if args[0] != MONITOR || (size != 40 && size != if wide { 104 } else { 72 }) {
                    threads.set_error(mem, 87)?;
                    return Ok(0);
                }
                mem.write(args[1], &profile::monitor(wide, size))?;
                Ok(1)
            }
            DisplayApi::FromPoint | DisplayApi::FromRect | DisplayApi::FromWindow => {
                if args[1] > 2 {
                    threads.set_error(mem, 87)?;
                    return Ok(0);
                }
                let intersects = match id {
                    DisplayApi::FromPoint => {
                        // POINT is an eight-byte by-value struct in one Win64 register.
                        let x = args[0] as u32 as i32;
                        let y = (args[0] >> 32) as u32 as i32;
                        (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y)
                    }
                    DisplayApi::FromRect => {
                        let r = rect(mem, args[0])?;
                        r[0] < r[2]
                            && r[1] < r[3]
                            && r[0] < WIDTH as i32
                            && r[2] > 0
                            && r[1] < HEIGHT as i32
                            && r[3] > 0
                    }
                    _ if args[0] == 0 => false,
                    _ => {
                        let Some(window) = self.windows.get(&args[0]) else {
                            threads.set_error(mem, 1400)?;
                            return Ok(0);
                        };
                        let r = window.rect;
                        r[0] < WIDTH as i32 && r[2] > 0 && r[1] < HEIGHT as i32 && r[3] > 0
                    }
                };
                Ok(if intersects || args[1] != 0 {
                    MONITOR
                } else {
                    0
                })
            }
            // CCD paths/modes are not provided. These APIs return Win32 status
            // directly; never zero the outputs to pretend there is a valid path.
            DisplayApi::ConfigSizes | DisplayApi::ConfigQuery | DisplayApi::ConfigDeviceInfo => {
                Ok(50)
            }
            _ => unreachable!(),
        }
    }
}

impl Runtime64 {
    pub(super) fn enumerate_monitors(
        &mut self,
        mem: &mut Memory,
        args: &[u64],
        returned: &Cpu64,
    ) -> Result<Enumeration, String> {
        let mut result = Enumeration {
            result: 1,
            calls: Vec::new(),
            release: Vec::new(),
        };
        if args[0] != 0 || args[2] == 0 {
            self.set_error(mem, if args[0] != 0 { 50 } else { 87 })?;
            result.result = 0;
            return Ok(result);
        }
        let mut r = [0i32, 0, WIDTH as i32, HEIGHT as i32];
        if args[1] != 0 {
            let clip = rect(mem, args[1])?;
            r = [
                r[0].max(clip[0]),
                r[1].max(clip[1]),
                r[2].min(clip[2]),
                r[3].min(clip[3]),
            ];
        }
        if r[0] >= r[2] || r[1] >= r[3] {
            return Ok(result);
        }
        if !self.threads.enabled {
            return Err("Win64 monitor callback requires a process executor".into());
        }
        mem.check(args[2], 1, READ | super::super::memory::EXECUTE)?;
        let frame = (returned.regs[4]
            .checked_sub(48)
            .ok_or("callback stack underflow")?
            & !15)
            + 8;
        mem.check(frame, 40, WRITE)?;
        let block = self.heap_alloc(mem, 16)?;
        if block == 0 {
            result.result = 0;
            return Ok(result);
        }
        let bytes: Vec<_> = r.into_iter().flat_map(i32::to_le_bytes).collect();
        mem.write(block, &bytes)?;
        result.calls.push((args[2], [MONITOR, 0, block, args[3]]));
        result.release.push(block);
        Ok(result)
    }
}
