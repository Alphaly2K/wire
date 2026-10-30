//! Version records use the guest profile and portable hierarchical comparisons.
use super::{
    call::Call,
    catalog::VersionApi as V,
    context,
    version_core::{self, Request, INVALID},
    ServiceError,
};
use crate::{
    environment::GuestEnvironment,
    machine::{Mode, State},
    memory::Memory,
};

fn request(memory: &Memory, pointer: u64, ansi: bool) -> Result<Request, ServiceError> {
    let size = if ansi { 156 } else { 284 };
    let mut bytes = [0; 284];
    memory.read(pointer, &mut bytes[..size])?;
    let dword = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let word = |at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    Ok(Request {
        size: if dword(0) == size as u32 { 284 } else { 0 },
        major: dword(4),
        minor: dword(8),
        build: dword(12),
        platform: dword(16),
        service_pack_major: word(size - 8),
        service_pack_minor: word(size - 6),
        suite_mask: word(size - 4),
        product_type: bytes[size - 2],
    })
}
pub(super) fn dispatch(
    id: V,
    call: &Call,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    let profile = if memory.mode() == Mode::X86 {
        GuestEnvironment::pe32()
    } else {
        GuestEnvironment::amd64()
    }
    .version();
    let a = &call.arguments;
    Ok(match id {
        V::GetVersion => profile.packed().into(),
        V::VerSetConditionMask => {
            let mask = a[1] as u32 & 255;
            a[0] | if mask == 0 {
                0
            } else {
                (a[2] & 7) << (mask.ilog2() * 3)
            }
        }
        V::VerifyVersionInfoA | V::VerifyVersionInfoW | V::RtlVerifyVersionInfo => {
            let result = version_core::verify(
                request(memory, a[0], id == V::VerifyVersionInfoA)?,
                a[1] as u32,
                a[2],
                profile,
            );
            if id == V::RtlVerifyVersionInfo {
                return Ok(result.into());
            }
            if result != 0 {
                memory.write_int(
                    context::error_address(state, memory.mode()),
                    4,
                    if result == INVALID { 160 } else { 1150 },
                )?;
            }
            u64::from(result == 0)
        }
        V::GetVersionExA | V::GetVersionExW | V::RtlGetVersion => {
            let ansi = id == V::GetVersionExA;
            let size = memory.read_int(a[0], 4)? as usize;
            if !(if ansi {
                matches!(size, 148 | 156)
            } else {
                matches!(size, 276 | 284)
            }) {
                if id == V::RtlGetVersion {
                    return Ok(INVALID.into());
                }
                memory.write_int(context::error_address(state, memory.mode()), 4, 87)?;
                return Ok(0);
            }
            let mut bytes = vec![0; size];
            for (n, value) in [
                size as u32,
                profile.major,
                profile.minor,
                profile.build,
                profile.platform,
            ]
            .into_iter()
            .enumerate()
            {
                bytes[n * 4..n * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
            if profile.service_pack_major != 0 {
                let text = format!("Service Pack {}", profile.service_pack_major);
                let data: Vec<_> = if ansi {
                    text.into_bytes()
                } else {
                    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
                };
                bytes[20..20 + data.len()].copy_from_slice(&data);
            }
            if size == 156 || size == 284 {
                bytes[size - 8..size - 6]
                    .copy_from_slice(&profile.service_pack_major.to_le_bytes());
                bytes[size - 6..size - 4]
                    .copy_from_slice(&profile.service_pack_minor.to_le_bytes());
                bytes[size - 4..size - 2].copy_from_slice(&profile.suite_mask.to_le_bytes());
                bytes[size - 2] = profile.product_type;
            }
            memory.write(a[0], &bytes)?;
            u64::from(id != V::RtlGetVersion)
        }
    })
}
