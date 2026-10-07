//! Checked guest Windows 10 (10.0.19045 workstation) version profile.
//! Version comparisons adapted from Wine 10.0 dlls/ntdll/version.c (LGPL-2.1-or-later).
//! Copyright 1997 Marcus Meissner; 1998 Patrik Stridvall;
//! 1998, 2003 Andreas Mohr; 1997, 2003 Alexandre Julliard.
//! Wire selects LGPL 2.1; source and notices are in third_party/wine-10.0/.
use super::{Memory, Runtime64};
const INVALID: u32 = 0xc000000d;
const MISMATCH: u32 = 0xc0000059;
pub(super) fn is_call(name: &str) -> bool {
    matches!(
        name,
        "RtlGetVersion" | "RtlVerifyVersionInfo" | "VerSetConditionMask"
    )
}
fn compare(current: u32, requested: u32, condition: u8) -> bool {
    match condition {
        1 => current == requested,
        2 => current > requested,
        3 => current >= requested,
        4 => current < requested,
        5 => current <= requested,
        _ => false,
    }
}
fn update(last: &mut u8, condition: u8) -> u8 {
    match *last {
        0 => *last = condition,
        1 if (1..=5).contains(&condition) => {
            *last = condition;
            return condition;
        }
        2 | 3 if (1..=3).contains(&condition) => return condition,
        4 | 5 if condition == 1 || (4..=5).contains(&condition) => return condition,
        _ => {}
    }
    if condition == 0 {
        *last |= 0x10;
    }
    *last & 15
}
fn verify(mem: &Memory, pointer: u64, mask: u32, conditions: u64) -> Result<u32, String> {
    let mut info = [0; 284];
    mem.read(pointer, &mut info)?;
    if u32::from_le_bytes(info[..4].try_into().unwrap()) != 284
        || mask == 0
        || mask & !0xff != 0
        || conditions == 0
    {
        return Ok(INVALID);
    }
    let requested = |bit| match bit {
        0 => u32::from_le_bytes(info[8..12].try_into().unwrap()),
        1 => u32::from_le_bytes(info[4..8].try_into().unwrap()),
        2 => u32::from_le_bytes(info[12..16].try_into().unwrap()),
        3 => u32::from_le_bytes(info[16..20].try_into().unwrap()),
        4 => u16::from_le_bytes(info[278..280].try_into().unwrap()) as u32,
        5 => u16::from_le_bytes(info[276..278].try_into().unwrap()) as u32,
        6 => u16::from_le_bytes(info[280..282].try_into().unwrap()) as u32,
        7 => info[282] as u32,
        _ => unreachable!(),
    };
    let condition = |bit| ((conditions >> (bit * 3)) & 7u64) as u8;
    if mask & 0x80 != 0 && !compare(1, requested(7), condition(7)) {
        return Ok(MISMATCH);
    }
    if mask & 0x40 != 0 {
        let expected = requested(6);
        let passed = match condition(6) {
            6 => 0x100 & expected == expected,
            7 => expected == 0 || 0x100 & expected != 0,
            _ => return Ok(INVALID),
        };
        if !passed {
            return Ok(MISMATCH);
        }
    }
    for (bit, value) in [(3, 2), (2, 19045)] {
        if mask & (1 << bit) != 0 && !compare(value, requested(bit), condition(bit)) {
            return Ok(MISMATCH);
        }
    }
    // Windows compares version/service-pack fields hierarchically. A greater
    // major version satisfies >= without independently checking the minor field.
    let mut last = 0;
    let mut passed = true;
    for (bit, value) in [(1, 10), (0, 0), (5, 0), (4, 0)] {
        if mask & (1 << bit) == 0 {
            continue;
        }
        let predicate = update(&mut last, condition(bit));
        passed = compare(value, requested(bit), predicate);
        if value != requested(bit) || !(1..=5).contains(&predicate) {
            break;
        }
    }
    Ok(if passed { 0 } else { MISMATCH })
}
impl Runtime64 {
    pub(super) fn version_call(
        &self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        match name {
            "GetVersion" => Ok((19045 << 16) | 10),
            "VerSetConditionMask" => {
                let mask = args[1] as u32 & 0xff;
                let shift = if mask == 0 { 0 } else { mask.ilog2() * 3 };
                Ok(args[0] | if mask == 0 { 0 } else { (args[2] & 7) << shift })
            }
            "RtlVerifyVersionInfo" | "VerifyVersionInfoW" => {
                let status = verify(mem, args[0], args[1] as u32, args[2])?;
                if name == "RtlVerifyVersionInfo" {
                    return Ok(status as u64);
                }
                if status != 0 {
                    self.set_error(mem, if status == INVALID { 160 } else { 1150 })?;
                }
                Ok((status == 0) as u64)
            }
            "RtlGetVersion" | "GetVersionExA" | "GetVersionExW" => {
                let size = mem.read_int(args[0], 4)? as usize;
                let ansi = name.ends_with('A');
                if !(if ansi {
                    matches!(size, 148 | 156)
                } else {
                    matches!(size, 276 | 284)
                }) {
                    if name == "RtlGetVersion" {
                        return Ok(INVALID as u64);
                    }
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                let mut info = vec![0; size];
                for (n, value) in [size as u32, 10, 0, 19045, 2].into_iter().enumerate() {
                    info[n * 4..n * 4 + 4].copy_from_slice(&value.to_le_bytes());
                }
                if matches!(size, 156 | 284) {
                    info[size - 4..size - 2].copy_from_slice(&0x100u16.to_le_bytes());
                    info[size - 2] = 1;
                }
                mem.write(args[0], &info)?;
                Ok((name != "RtlGetVersion") as u64)
            }
            _ => unreachable!(),
        }
    }
}
