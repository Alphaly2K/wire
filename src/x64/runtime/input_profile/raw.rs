//! Win64 raw-input layout and checked copy-out over the logical device owner.
use super::{InputResult, Memory, RawInputApi, User};
use crate::{
    win32::user::raw_input::{Registration, KEYBOARD, MOUSE},
    x64::memory::WRITE,
};

fn error(code: u32) -> InputResult {
    InputResult::error(u32::MAX as u64, code)
}
fn copy_list(
    mem: &mut Memory,
    a: &[u64],
    bytes: &[u8],
    count: u32,
    stride: u64,
) -> Result<InputResult, String> {
    if a[2] != stride || a[1] == 0 {
        return Ok(error(87));
    }
    mem.check(a[1], 4, WRITE)?;
    if a[0] == 0 {
        mem.write_int(a[1], 4, count as u64)?;
        return Ok(InputResult::value(0));
    }
    if mem.read_int(a[1], 4)? < count as u64 {
        mem.write_int(a[1], 4, count as u64)?;
        return Ok(error(122));
    }
    mem.check(a[0], bytes.len(), WRITE)?;
    mem.write(a[0], bytes)?;
    mem.write_int(a[1], 4, count as u64)?;
    Ok(InputResult::value(count as u64))
}

impl User {
    pub(super) fn raw_input_call(
        &mut self,
        mem: &mut Memory,
        id: RawInputApi,
        a: &[u64],
        tid: u64,
    ) -> Result<InputResult, String> {
        use RawInputApi::*;
        match id {
            Register => {
                if a[2] != 16 || a[1] == 0 || a[1] > 1024 || a[0] & 3 != 0 {
                    return Ok(InputResult::error(0, 87));
                }
                let mut data = vec![0; a[1] as usize * 16];
                mem.read(a[0], &mut data)?;
                let mut registrations = self.raw_input.registrations.clone();
                for d in data.as_chunks::<16>().0 {
                    let page = u16::from_le_bytes(d[..2].try_into().unwrap());
                    let usage = u16::from_le_bytes(d[2..4].try_into().unwrap());
                    let flags = u32::from_le_bytes(d[4..8].try_into().unwrap());
                    let target = u64::from_le_bytes(d[8..16].try_into().unwrap());
                    if page == 0 || usage == 0 || (flags & 1 != 0 && (flags != 1 || target != 0)) {
                        return Ok(InputResult::error(0, 87));
                    }
                    if flags == 1 {
                        if page == 1 {
                            registrations.remove(&usage);
                        }
                        continue;
                    }
                    if page != 1 || !matches!(usage, 2 | 6) || !matches!(flags & !0x2000, 0 | 0x30)
                    {
                        return Ok(InputResult::error(0, 50));
                    }
                    if target != 0 && !self.windows.contains_key(&target) {
                        return Ok(InputResult::error(0, 1400));
                    }
                    registrations.insert(
                        usage,
                        Registration {
                            usage,
                            flags,
                            target,
                        },
                    );
                }
                self.raw_input.registrations = registrations;
                Ok(InputResult::value(1))
            }
            Registered => {
                let mut bytes = Vec::new();
                for r in self.raw_input.registrations.values() {
                    bytes.extend(1u16.to_le_bytes());
                    bytes.extend(r.usage.to_le_bytes());
                    bytes.extend(r.flags.to_le_bytes());
                    bytes.extend(r.target.to_le_bytes());
                }
                copy_list(
                    mem,
                    a,
                    &bytes,
                    self.raw_input.registrations.len() as u32,
                    16,
                )
            }
            DeviceList => {
                let mut bytes = Vec::new();
                for (handle, kind) in [(MOUSE, 0u32), (KEYBOARD, 1)] {
                    bytes.extend(handle.to_le_bytes());
                    bytes.extend(kind.to_le_bytes());
                    bytes.extend([0; 4]);
                }
                copy_list(mem, a, &bytes, 2, 16)
            }
            Data => {
                if a[4] != 24 {
                    return Ok(error(87));
                }
                let Some(packet) = self.raw_input.packets.get(&a[0]) else {
                    return Ok(error(6));
                };
                if a[3] == 0 || a[2] & 3 != 0 {
                    return Ok(error(87));
                }
                let mut bytes = packet.bytes();
                match a[1] {
                    0x10000003 => {}
                    0x10000005 => bytes.truncate(24),
                    _ => return Ok(error(87)),
                }
                mem.check(a[3], 4, WRITE)?;
                if a[2] == 0 {
                    mem.write_int(a[3], 4, bytes.len() as u64)?;
                    return Ok(InputResult::value(0));
                }
                if mem.read_int(a[3], 4)? < bytes.len() as u64 {
                    mem.write_int(a[3], 4, bytes.len() as u64)?;
                    return Ok(error(122));
                }
                mem.check(a[2], bytes.len(), WRITE)?;
                mem.write(a[2], &bytes)?;
                mem.write_int(a[3], 4, bytes.len() as u64)?;
                crate::host::ui::trace_input("raw-read", packet.window, 0xff, 0, a[0], tid);
                Ok(InputResult::value(bytes.len() as u64))
            }
            Buffer => self.raw_input_buffer(mem, a, tid),
            DeviceInfoA | DeviceInfoW => {
                if !matches!(a[0], MOUSE | KEYBOARD) {
                    return Ok(error(6));
                }
                if a[3] == 0 {
                    return Ok(error(87));
                }
                let mouse = a[0] == MOUSE;
                let bytes = match a[1] {
                    0x20000007 => {
                        let name = if mouse {
                            "\\\\?\\WIRE#MOUSE#1\0"
                        } else {
                            "\\\\?\\WIRE#KEYBOARD#1\0"
                        };
                        if id == DeviceInfoW {
                            name.encode_utf16()
                                .flat_map(u16::to_le_bytes)
                                .collect::<Vec<_>>()
                        } else {
                            name.as_bytes().to_vec()
                        }
                    }
                    0x2000000b => {
                        let mut data = vec![0; 32];
                        let fields = if mouse {
                            vec![32u32, 0, 1, 5, 60, 1]
                        } else {
                            vec![32, 1, 4, 0, 1, 12, 3, 101]
                        };
                        for (n, v) in fields.into_iter().enumerate() {
                            data[n * 4..n * 4 + 4].copy_from_slice(&v.to_le_bytes());
                        }
                        data
                    }
                    _ => return Ok(error(87)),
                };
                let width = if a[1] == 0x20000007 && id == DeviceInfoW {
                    2
                } else {
                    1
                };
                let count = bytes.len() / width;
                mem.check(a[3], 4, WRITE)?;
                if a[2] == 0 {
                    mem.write_int(a[3], 4, count as u64)?;
                    return Ok(InputResult::value(0));
                }
                if mem.read_int(a[3], 4)? < count as u64 {
                    mem.write_int(a[3], 4, count as u64)?;
                    return Ok(error(122));
                }
                if a[1] == 0x2000000b && mem.read_int(a[2], 4)? != 32 {
                    return Ok(error(87));
                }
                mem.check(a[2], bytes.len(), WRITE)?;
                mem.write(a[2], &bytes)?;
                mem.write_int(a[3], 4, count as u64)?;
                Ok(InputResult::value(count as u64))
            }
        }
    }
    fn raw_input_buffer(
        &mut self,
        mem: &mut Memory,
        a: &[u64],
        tid: u64,
    ) -> Result<InputResult, String> {
        if a[2] != 24 || a[1] == 0 || a[0] & 7 != 0 {
            return Ok(error(87));
        }
        mem.check(a[1], 4, WRITE)?;
        let pending: Vec<_> = self
            .raw_input
            .packets
            .iter()
            .filter(|(_, p)| p.queued && p.tid == tid)
            .collect();
        let first_size = pending.first().map_or(0, |(_, p)| p.bytes().len());
        if a[0] == 0 || first_size == 0 {
            mem.write_int(a[1], 4, first_size as u64)?;
            return Ok(InputResult::value(0));
        }
        let capacity = mem.read_int(a[1], 4)? as usize;
        if capacity < first_size {
            mem.write_int(a[1], 4, first_size as u64)?;
            return Ok(error(122));
        }
        let mut bytes = Vec::new();
        let mut handles = Vec::new();
        for (&handle, packet) in pending {
            let data = packet.bytes();
            let next = bytes.len().next_multiple_of(8);
            if next + data.len() > capacity {
                break;
            }
            bytes.resize(next, 0);
            bytes.extend(data);
            handles.push(handle);
        }
        mem.check(a[0], bytes.len(), WRITE)?;
        mem.write(a[0], &bytes)?;
        mem.write_int(a[1], 4, bytes.len() as u64)?;
        self.events
            .retain(|m| m.event.message != 0xff || !handles.contains(&m.event.lparam));
        for handle in &handles {
            if let Some(packet) = self.raw_input.packets.remove(handle) {
                if packet.no_legacy {
                    let [message, key, lp] = packet.input;
                    self.keyboard.consumed64(tid, message, key, lp);
                }
                crate::host::ui::trace_input("raw-buffer", packet.window, 0xff, 0, *handle, tid);
            }
        }
        Ok(InputResult::value(handles.len() as u64))
    }
}
