//! One virtual 1024x768/32-bit/60-Hz desktop; Win64 handles and checked outputs.
//! This describes the guest display profile, not a native GPU or swap chain.
use super::{Cpu64, Memory, Runtime64, READ, WRITE};
const MONITOR: u64 = super::TEB + 0x600008;
const DEVICE: &str = "\\\\.\\DISPLAY1";
pub(super) fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "EnumDisplayDevicesA"
        | "EnumDisplayDevicesW"
        | "EnumDisplaySettingsExA"
        | "EnumDisplaySettingsExW"
        | "EnumDisplayMonitors" => 4,
        "EnumDisplaySettingsA" | "EnumDisplaySettingsW" => 3,
        "MonitorFromPoint" | "MonitorFromRect" | "MonitorFromWindow" | "GetMonitorInfoA"
        | "GetMonitorInfoW" => 2,
        "GetDisplayConfigBufferSizes" => 3,
        "QueryDisplayConfig" => 6,
        "DisplayConfigGetDeviceInfo" => 1,
        _ => return None,
    })
}
pub(super) fn dwords(name: &str) -> &'static [usize] {
    match name {
        "EnumDisplayDevicesA"
        | "EnumDisplayDevicesW"
        | "EnumDisplaySettingsExA"
        | "EnumDisplaySettingsExW" => &[1, 3],
        "EnumDisplaySettingsA"
        | "EnumDisplaySettingsW"
        | "MonitorFromPoint"
        | "MonitorFromRect"
        | "MonitorFromWindow" => &[1],
        "GetDisplayConfigBufferSizes" | "QueryDisplayConfig" => &[0],
        _ => &[],
    }
}
fn text(bytes: &mut [u8], at: usize, value: &str, wide: bool) {
    let data = if wide {
        value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    } else {
        value.as_bytes().to_vec()
    };
    bytes[at..at + data.len()].copy_from_slice(&data);
}
fn rect(mem: &Memory, pointer: u64) -> Result<[i32; 4], String> {
    let mut bytes = [0; 16];
    mem.read(pointer, &mut bytes)?;
    Ok(std::array::from_fn(|i| {
        i32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap())
    }))
}
pub(super) struct Enumeration {
    pub result: u64,
    pub calls: Vec<(u64, [u64; 4])>,
    pub release: Vec<u64>,
}
impl Runtime64 {
    pub(super) fn display_call(
        &self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        let wide = name.ends_with('W');
        if name.starts_with("EnumDisplay") {
            let device = if args[0] == 0 {
                String::new()
            } else {
                Self::path_name(mem, args[0], wide)?
            };
            if name.starts_with("EnumDisplayDevices") {
                let size = if wide { 840 } else { 424 };
                if mem.read_int(args[2], 4)? != size || args[3] & !1 != 0 {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                if args[1] != 0 || (!device.is_empty() && !device.eq_ignore_ascii_case(DEVICE)) {
                    return Ok(0);
                }
                let adapter = device.is_empty();
                let mut bytes = vec![0; size as usize];
                bytes[..4].copy_from_slice(&(size as u32).to_le_bytes());
                text(
                    &mut bytes,
                    4,
                    &if adapter {
                        DEVICE.to_owned()
                    } else {
                        format!("{DEVICE}\\Monitor0")
                    },
                    wide,
                );
                text(
                    &mut bytes,
                    if wide { 68 } else { 36 },
                    if adapter {
                        "Wire virtual display"
                    } else {
                        "Wire virtual monitor"
                    },
                    wide,
                );
                let flags = if wide { 324 } else { 164 };
                bytes[flags..flags + 4]
                    .copy_from_slice(&(if adapter { 5u32 } else { 3 }).to_le_bytes());
                // No Windows registry/PnP key or physical device ID is claimed.
                mem.write(args[2], &bytes)?;
                return Ok(1);
            }
            if (!device.is_empty() && !device.eq_ignore_ascii_case(DEVICE))
                || !matches!(args[1], 0 | 0xffffffff | 0xfffffffe)
            {
                return Ok(0);
            }
            if name.contains("Ex") && args[3] & !3 != 0 {
                self.set_error(mem, 87)?;
                return Ok(0);
            }
            let size_offset = if wide { 68 } else { 36 };
            let size = mem.read_int(
                args[2].checked_add(size_offset).ok_or("DEVMODE overflow")?,
                2,
            )? as usize;
            if !(if wide { 188..=220 } else { 124..=156 }).contains(&size) {
                self.set_error(mem, 87)?;
                return Ok(0);
            }
            let mut bytes = vec![0; size];
            text(&mut bytes, 0, DEVICE, wide);
            let offset = size_offset as usize;
            bytes[offset - 4..offset - 2].copy_from_slice(&0x401u16.to_le_bytes());
            bytes[offset..offset + 2].copy_from_slice(&(size as u16).to_le_bytes());
            bytes[offset + 4..offset + 8].copy_from_slice(&0x005c0020u32.to_le_bytes());
            let bits = if wide { 168 } else { 104 };
            for (i, value) in [32u32, 1024, 768, 0, 60].into_iter().enumerate() {
                bytes[bits + 4 * i..bits + 4 * i + 4].copy_from_slice(&value.to_le_bytes());
            }
            mem.write(args[2], &bytes)?;
            return Ok(1);
        }
        match name {
            "GetMonitorInfoA" | "GetMonitorInfoW" => {
                let size = mem.read_int(args[1], 4)? as usize;
                if args[0] != MONITOR || (size != 40 && size != if wide { 104 } else { 72 }) {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                let mut bytes = vec![0; size];
                for (i, value) in [size as u32, 0, 0, 1024, 768, 0, 0, 1024, 768, 1]
                    .into_iter()
                    .enumerate()
                {
                    bytes[4 * i..4 * i + 4].copy_from_slice(&value.to_le_bytes());
                }
                if size != 40 {
                    text(&mut bytes, 40, DEVICE, wide);
                }
                mem.write(args[1], &bytes)?;
                Ok(1)
            }
            "MonitorFromPoint" | "MonitorFromRect" | "MonitorFromWindow" => {
                if args[1] > 2 {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                let intersects = match name {
                    "MonitorFromPoint" => {
                        // POINT is an eight-byte by-value struct in one Win64 register.
                        let x = args[0] as u32 as i32;
                        let y = (args[0] >> 32) as u32 as i32;
                        (0..1024).contains(&x) && (0..768).contains(&y)
                    }
                    "MonitorFromRect" => {
                        let r = rect(mem, args[0])?;
                        r[0] < r[2]
                            && r[1] < r[3]
                            && r[0] < 1024
                            && r[2] > 0
                            && r[1] < 768
                            && r[3] > 0
                    }
                    _ if args[0] == 0 => false,
                    _ => {
                        let Some(window) = self.user.windows.get(&args[0]) else {
                            self.set_error(mem, 1400)?;
                            return Ok(0);
                        };
                        let r = window.rect;
                        r[0] < 1024 && r[2] > 0 && r[1] < 768 && r[3] > 0
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
            "GetDisplayConfigBufferSizes" | "QueryDisplayConfig" | "DisplayConfigGetDeviceInfo" => {
                Ok(50)
            }
            _ => unreachable!(),
        }
    }
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
        let mut r = [0i32, 0, 1024, 768];
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
