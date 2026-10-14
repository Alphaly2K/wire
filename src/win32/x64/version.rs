//! Checked guest Windows 10 (10.0.19045 workstation) version profile.
//! Version comparisons adapted from Wine 10.0 dlls/ntdll/version.c (LGPL-2.1-or-later).
//! Copyright 1997 Marcus Meissner; 1998 Patrik Stridvall;
//! 1998, 2003 Andreas Mohr; 1997, 2003 Alexandre Julliard.
//! Wire selects LGPL 2.1; source and notices are in third_party/wine-10.0/.
use crate::{
    runtime::environment::GuestEnvironment,
    win32::{
        catalog::VersionApi,
        kernel::version::{self, Request, INVALID},
    },
    x64::memory::Memory,
};

fn read_request(mem: &Memory, pointer: u64) -> Result<Request, String> {
    // Read the entire structure before validating size/masks, as on the legacy route.
    let mut info = [0; 284];
    mem.read(pointer, &mut info)?;
    let dword = |at: usize| u32::from_le_bytes(info[at..at + 4].try_into().unwrap());
    let word = |at: usize| u16::from_le_bytes(info[at..at + 2].try_into().unwrap());
    Ok(Request {
        size: dword(0),
        major: dword(4),
        minor: dword(8),
        build: dword(12),
        platform: dword(16),
        service_pack_major: word(276),
        service_pack_minor: word(278),
        suite_mask: word(280),
        product_type: info[282],
    })
}

pub(crate) fn call(
    mem: &mut Memory,
    id: VersionApi,
    args: &[u64],
    environment: &GuestEnvironment,
    error_slot: u64,
) -> Result<u64, String> {
    let profile = environment.version();
    match id {
        VersionApi::GetVersion => Ok(profile.packed() as u64),
        VersionApi::VerSetConditionMask => {
            let mask = args[1] as u32 & 0xff;
            let shift = if mask == 0 { 0 } else { mask.ilog2() * 3 };
            Ok(args[0] | if mask == 0 { 0 } else { (args[2] & 7) << shift })
        }
        VersionApi::RtlVerifyVersionInfo | VersionApi::VerifyVersionInfoW => {
            let status = version::verify(
                read_request(mem, args[0])?,
                args[1] as u32,
                args[2],
                profile,
            );
            if id == VersionApi::RtlVerifyVersionInfo {
                return Ok(status as u64);
            }
            if status != 0 {
                mem.write_int(error_slot, 4, if status == INVALID { 160 } else { 1150 })?;
            }
            Ok((status == 0) as u64)
        }
        VersionApi::RtlGetVersion | VersionApi::GetVersionExA | VersionApi::GetVersionExW => {
            let size = mem.read_int(args[0], 4)? as usize;
            let ansi = id == VersionApi::GetVersionExA;
            if !(if ansi {
                matches!(size, 148 | 156)
            } else {
                matches!(size, 276 | 284)
            }) {
                if id == VersionApi::RtlGetVersion {
                    return Ok(INVALID as u64);
                }
                mem.write_int(error_slot, 4, 87)?;
                return Ok(0);
            }
            let mut info = vec![0; size];
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
                info[n * 4..n * 4 + 4].copy_from_slice(&value.to_le_bytes());
            }
            if matches!(size, 156 | 284) {
                info[size - 4..size - 2].copy_from_slice(&profile.suite_mask.to_le_bytes());
                info[size - 2] = profile.product_type;
            }
            mem.write(args[0], &info)?;
            Ok((id != VersionApi::RtlGetVersion) as u64)
        }
        VersionApi::VerifyVersionInfoA => unreachable!("unbound ANSI version verification"),
    }
}
