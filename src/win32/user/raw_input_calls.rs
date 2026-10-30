//! Shared raw-input registration, layouts and checked packet copy-out.
use super::{
    input_profile::InputResult,
    raw_input::{Packet, RawInput, Registration, KEYBOARD, MOUSE},
};
use crate::win32::catalog::RawInputApi;

pub(crate) trait Backend {
    type Error;
    fn width(&self) -> usize;
    fn check_write(&self, pointer: u64, size: usize) -> Result<(), Self::Error>;
    fn read(&self, pointer: u64, output: &mut [u8]) -> Result<(), Self::Error>;
    fn write(&mut self, pointer: u64, bytes: &[u8]) -> Result<(), Self::Error>;
    fn window_exists(&self, handle: u64) -> bool;
    fn consume(&mut self, packets: &[(u64, Packet)]);
    fn packet_read(&mut self, _handle: u64, _packet: &Packet, _tid: u64) {}
    fn read_int(&self, pointer: u64, size: usize) -> Result<u64, Self::Error> {
        let mut bytes = [0; 8];
        self.read(pointer, &mut bytes[..size])?;
        Ok(u64::from_le_bytes(bytes))
    }
    fn write_int(&mut self, pointer: u64, size: usize, value: u64) -> Result<(), Self::Error> {
        self.write(pointer, &value.to_le_bytes()[..size])
    }
}
fn error(code: u32) -> InputResult {
    InputResult::error(u32::MAX as u64, code)
}
fn copy_list<B: Backend>(
    mem: &mut B,
    a: &[u64],
    bytes: &[u8],
    count: u32,
    stride: u64,
) -> Result<InputResult, B::Error> {
    if a[2] != stride || a[1] == 0 {
        return Ok(error(87));
    }
    mem.check_write(a[1], 4)?;
    if a[0] == 0 {
        mem.write_int(a[1], 4, count as u64)?;
        return Ok(InputResult::value(0));
    }
    if mem.read_int(a[1], 4)? < count as u64 {
        mem.write_int(a[1], 4, count as u64)?;
        return Ok(error(122));
    }
    mem.check_write(a[0], bytes.len())?;
    mem.write(a[0], bytes)?;
    mem.write_int(a[1], 4, count as u64)?;
    Ok(InputResult::value(count as u64))
}

pub(crate) fn call<B: Backend>(
    owner: &mut RawInput,
    mem: &mut B,
    id: RawInputApi,
    a: &[u64],
    tid: u64,
) -> Result<InputResult, B::Error> {
    use RawInputApi::*;
    let width = mem.width();
    let stride = 8 + width;
    let header = 8 + width * 2;
    match id {
        Register => {
            if a[2] != stride as u64 || a[1] == 0 || a[1] > 1024 || a[0] & 3 != 0 {
                return Ok(InputResult::error(0, 87));
            }
            let mut data = vec![0; a[1] as usize * stride];
            mem.read(a[0], &mut data)?;
            let mut registrations = owner.registrations.clone();
            for d in data.chunks_exact(stride) {
                let page = u16::from_le_bytes(d[..2].try_into().unwrap());
                let usage = u16::from_le_bytes(d[2..4].try_into().unwrap());
                let flags = u32::from_le_bytes(d[4..8].try_into().unwrap());
                let mut target = [0; 8];
                target[..width].copy_from_slice(&d[8..stride]);
                let target = u64::from_le_bytes(target);
                if page == 0 || usage == 0 || (flags & 1 != 0 && (flags != 1 || target != 0)) {
                    return Ok(InputResult::error(0, 87));
                }
                if flags == 1 {
                    if page == 1 {
                        registrations.remove(&usage);
                    }
                    continue;
                }
                if page != 1 || !matches!(usage, 2 | 6) || !matches!(flags & !0x2000, 0 | 0x30) {
                    return Ok(InputResult::error(0, 50));
                }
                if target != 0 && !mem.window_exists(target) {
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
            owner.registrations = registrations;
            Ok(InputResult::value(1))
        }
        Registered => {
            let mut bytes = Vec::new();
            for r in owner.registrations.values() {
                bytes.extend(1u16.to_le_bytes());
                bytes.extend(r.usage.to_le_bytes());
                bytes.extend(r.flags.to_le_bytes());
                bytes.extend_from_slice(&r.target.to_le_bytes()[..width]);
            }
            copy_list(
                mem,
                a,
                &bytes,
                owner.registrations.len() as u32,
                stride as u64,
            )
        }
        DeviceList => {
            let mut bytes = Vec::new();
            for (handle, kind) in [(MOUSE, 0u32), (KEYBOARD, 1)] {
                bytes.extend_from_slice(&handle.to_le_bytes()[..width]);
                bytes.extend(kind.to_le_bytes());
                if width == 8 {
                    bytes.extend([0; 4]);
                }
            }
            copy_list(mem, a, &bytes, 2, if width == 8 { 16 } else { 8 })
        }
        Data => {
            if a[4] != header as u64 {
                return Ok(error(87));
            }
            let Some(packet) = owner.packets.get(&a[0]) else {
                return Ok(error(6));
            };
            if a[3] == 0 || a[2] & 3 != 0 {
                return Ok(error(87));
            }
            let mut bytes = packet.bytes(width);
            match a[1] {
                0x10000003 => {}
                0x10000005 => bytes.truncate(header),
                _ => return Ok(error(87)),
            }
            mem.check_write(a[3], 4)?;
            if a[2] == 0 {
                mem.write_int(a[3], 4, bytes.len() as u64)?;
                return Ok(InputResult::value(0));
            }
            if mem.read_int(a[3], 4)? < bytes.len() as u64 {
                mem.write_int(a[3], 4, bytes.len() as u64)?;
                return Ok(error(122));
            }
            mem.check_write(a[2], bytes.len())?;
            mem.write(a[2], &bytes)?;
            mem.write_int(a[3], 4, bytes.len() as u64)?;
            mem.packet_read(a[0], packet, tid);
            Ok(InputResult::value(bytes.len() as u64))
        }
        Buffer => buffer(owner, mem, a, tid),
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
            mem.check_write(a[3], 4)?;
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
            mem.check_write(a[2], bytes.len())?;
            mem.write(a[2], &bytes)?;
            mem.write_int(a[3], 4, count as u64)?;
            Ok(InputResult::value(count as u64))
        }
    }
}
fn buffer<B: Backend>(
    owner: &mut RawInput,
    mem: &mut B,
    a: &[u64],
    tid: u64,
) -> Result<InputResult, B::Error> {
    let width = mem.width();
    if a[2] != (8 + width * 2) as u64 || a[1] == 0 || a[0] & (width as u64 - 1) != 0 {
        return Ok(error(87));
    }
    mem.check_write(a[1], 4)?;
    let pending: Vec<_> = owner
        .packets
        .iter()
        .filter(|(_, p)| p.queued && p.tid == tid)
        .collect();
    let first_size = pending.first().map_or(0, |(_, p)| p.size(width));
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
        let data = packet.bytes(width);
        let next = bytes.len().next_multiple_of(width);
        if next + data.len() > capacity {
            break;
        }
        bytes.resize(next, 0);
        bytes.extend(data);
        handles.push(handle);
    }
    mem.check_write(a[0], bytes.len())?;
    mem.write(a[0], &bytes)?;
    mem.write_int(a[1], 4, bytes.len() as u64)?;
    let packets: Vec<_> = handles
        .iter()
        .map(|handle| (*handle, owner.packets.remove(handle).unwrap()))
        .collect();
    mem.consume(&packets);
    Ok(InputResult::value(handles.len() as u64))
}
