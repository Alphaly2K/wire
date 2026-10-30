//! Virtual hardware queries use the JIT CPU profile and the live VMM owner.
use super::{
    call::Call,
    catalog::SystemProfileApi as S,
    context,
    modules::{name, Failure},
    system_topology, ServiceError,
};
use crate::{
    filesystem::Files,
    machine::{Mode, State},
    memory::{Memory, PAGE, WRITE},
};

pub(super) fn dispatch(
    id: S,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    files: &Files,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    let wide = memory.mode() == Mode::X64;
    match id {
        S::LanguageNameA | S::LanguageNameW => {
            let text = match a[0] as u32 {
                0 => "Language Neutral",
                9 => "English",
                0x11 => "Japanese",
                0x409 => "English (United States)",
                0x411 => "Japanese (Japan)",
                0x7f => "Invariant Language (Invariant Country)",
                _ => return Ok(0),
            };
            if a[2] == 0 {
                return Ok(0);
            }
            let count = text.len().min(a[2] as u32 as usize - 1);
            let bytes = if id == S::LanguageNameW {
                text[..count]
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
            } else {
                text.bytes().take(count).chain(Some(0)).collect()
            };
            memory.write(a[1], &bytes)?;
            Ok(count as u64)
        }
        S::FlushWrites => Ok(0),
        S::ProcessorNumber => {
            memory.write(a[0], &[0; 4])?;
            Ok(0)
        }
        S::LogicalDrivesA | S::LogicalDrivesW => {
            let wide = id == S::LogicalDrivesW;
            let bytes = if wide {
                b"C\0:\0\\\0\0\0\0\0".as_slice()
            } else {
                b"C:\\\0\0".as_slice()
            };
            if a[0] < 5 {
                return Ok(5);
            }
            memory.write(a[1], bytes)?;
            Ok(4)
        }
        S::VolumeA | S::VolumeW => volume(id, a, state, memory, files),
        S::DiskSpaceA | S::DiskSpaceW => {
            let path = if a[0] == 0 {
                Ok(files.cwd().to_owned())
            } else {
                name(memory, a[0], id == S::DiskSpaceW)
            };
            let code = match path {
                Err(Failure::Fault(fault)) => return Err(fault.into()),
                Err(Failure::Windows(code)) => code,
                Ok(path) => match files.metadata(&path) {
                    Ok(metadata) if metadata.attributes & 0x10 != 0 => 50,
                    Ok(_) => 267,
                    Err(2) => 3,
                    Err(code) => code,
                },
            };
            error(state, memory, code.into())
        }
        S::DriveTypeA | S::DriveTypeW => {
            if a[0] == 0 {
                return Ok(3);
            }
            let path = match name(memory, a[0], id == S::DriveTypeW) {
                Ok(path) => path,
                Err(Failure::Fault(fault)) => return Err(fault.into()),
                Err(Failure::Windows(_)) => return Ok(1),
            };
            let directory = path.ends_with(['\\', '/'])
                && files
                    .metadata(&path)
                    .is_ok_and(|m| m.attributes & 0x10 != 0);
            Ok(if directory { 3 } else { 1 })
        }
        S::PowerStatus => {
            memory.write(
                a[0],
                &[1, 128, 255, 0, 255, 255, 255, 255, 255, 255, 255, 255],
            )?;
            Ok(1)
        }
        S::SystemInfo => {
            let mut data = [0; 48];
            let mut put = |at: usize, value: u64, size: usize| {
                data[at..at + size].copy_from_slice(&value.to_le_bytes()[..size]);
            };
            let pointer = if wide { 8 } else { 4 };
            put(0, if wide { 9 } else { 0 }, 2);
            put(4, PAGE, 4);
            put(8, 65536, pointer);
            put(
                8 + pointer,
                if wide { (1 << 47) - 65537 } else { 0x7ffeffff },
                pointer,
            );
            put(8 + 2 * pointer, 1, pointer);
            let scalar = 8 + 3 * pointer;
            put(scalar, 1, 4);
            put(scalar + 4, if wide { 8664 } else { 586 }, 4);
            put(scalar + 8, 65536, 4);
            put(
                scalar + 12,
                u64::from((crate::cpu::cpuid(1, 0)[0] >> 8) & 15),
                2,
            );
            memory.write(a[0], &data[..if wide { 48 } else { 36 }])?;
            Ok(0)
        }
        S::MemoryStatusEx => {
            if memory.read_int(a[0], 4)? != 64 {
                return error(state, memory, 87);
            }
            memory.write(a[0], &memory.memory_status())?;
            Ok(1)
        }
        S::MemoryStatus => {
            let status = memory.memory_status();
            let mut data = [0; 32];
            data[..4].copy_from_slice(&32u32.to_le_bytes());
            data[4..8].copy_from_slice(&status[4..8]);
            for n in 0..6 {
                let value = u64::from_le_bytes(status[8 + n * 8..16 + n * 8].try_into().unwrap());
                data[8 + n * 4..12 + n * 4]
                    .copy_from_slice(&(value.min(u32::MAX as u64) as u32).to_le_bytes());
            }
            memory.write(a[0], &data)?;
            Ok(0)
        }
        S::ProcessorTopology => {
            let Some(data) = system_topology::encode(a[0] as u32, wide) else {
                return error(state, memory, 87);
            };
            memory.check(a[2], 4, WRITE)?;
            let capacity = memory.read_int(a[2], 4)?;
            if capacity < data.len() as u64 {
                // Both outputs must be writable before publishing the size.
                memory.check(context::error_address(state, memory.mode()), 4, WRITE)?;
                memory.write_int(a[2], 4, data.len() as u64)?;
                return error(state, memory, 122);
            }
            if !data.is_empty() {
                memory.check(a[1], data.len(), WRITE)?;
                memory.write(a[1], &data)?;
            }
            memory.write_int(a[2], 4, data.len() as u64)?;
            Ok(1)
        }
    }
}

fn volume(
    id: S,
    a: &[u64; 16],
    state: &State,
    memory: &mut Memory,
    files: &Files,
) -> Result<u64, ServiceError> {
    let wide = id == S::VolumeW;
    let root = if a[0] == 0 {
        Ok("C:\\".to_owned())
    } else {
        name(memory, a[0], wide)
    };
    let root = match root {
        Ok(p) => p,
        Err(Failure::Fault(f)) => return Err(f.into()),
        Err(Failure::Windows(c)) => return error(state, memory, c.into()),
    };
    if !root.ends_with(['\\', '/']) {
        return error(state, memory, 123);
    }
    match files.resolve(&root) {
        Ok(p) if p.eq_ignore_ascii_case("C:\\") => {}
        Ok(_) => return error(state, memory, 144),
        Err(code) => return error(state, memory, code.into()),
    }
    let texts = ["Wire", "WireFS"];
    let mut outputs = Vec::new();
    for (index, text) in texts.into_iter().enumerate() {
        let pointer = a[if index == 0 { 1 } else { 6 }];
        let capacity = a[if index == 0 { 2 } else { 7 }];
        if pointer != 0 {
            if capacity < (text.len() + 1) as u64 {
                return error(state, memory, 234);
            }
            let bytes = if wide {
                text.encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>()
            } else {
                text.bytes().chain(Some(0)).collect()
            };
            outputs.push((pointer, bytes));
        }
    }
    for (&pointer, value) in a[3..6].iter().zip([1u32, 32765, 6]) {
        if pointer != 0 {
            outputs.push((pointer, value.to_le_bytes().to_vec()));
        }
    }
    for (p, bytes) in &outputs {
        memory.check(*p, bytes.len(), WRITE)?;
    }
    for (p, bytes) in outputs {
        memory.write(p, &bytes)?;
    }
    Ok(1)
}

fn error(state: &State, memory: &mut Memory, code: u64) -> Result<u64, ServiceError> {
    memory.write_int(context::error_address(state, memory.mode()), 4, code)?;
    Ok(0)
}
