//! Pure Windows version comparisons; guest structure decoding stays in adapters.
//! Version comparisons adapted from Wine 10.0 dlls/ntdll/version.c (LGPL-2.1-or-later).
//! Copyright 1997 Marcus Meissner; 1998 Patrik Stridvall;
//! 1998, 2003 Andreas Mohr; 1997, 2003 Alexandre Julliard.
//! Wire selects LGPL 2.1; source and notices are in third_party/wine-10.0/.
pub(crate) const INVALID: u32 = 0xc000000d;
const MISMATCH: u32 = 0xc0000059;
#[derive(Clone, Copy)]
pub(crate) struct Request {
    pub size: u32,
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub platform: u32,
    pub service_pack_major: u16,
    pub service_pack_minor: u16,
    pub suite_mask: u16,
    pub product_type: u8,
}
impl Request {
    fn field(self, bit: u32) -> u32 {
        match bit {
            0 => self.minor,
            1 => self.major,
            2 => self.build,
            3 => self.platform,
            4 => self.service_pack_minor as u32,
            5 => self.service_pack_major as u32,
            6 => self.suite_mask as u32,
            7 => self.product_type as u32,
            _ => unreachable!(),
        }
    }
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
pub(crate) fn verify(
    requested: Request,
    mask: u32,
    conditions: u64,
    version: super::WindowsVersion,
) -> u32 {
    if requested.size != 284 || mask == 0 || mask & !0xff != 0 || conditions == 0 {
        return INVALID;
    }
    let requested = |bit| requested.field(bit);
    let condition = |bit| ((conditions >> (bit * 3)) & 7u64) as u8;
    if mask & 0x80 != 0 && !compare(version.product_type as u32, requested(7), condition(7)) {
        return MISMATCH;
    }
    if mask & 0x40 != 0 {
        let expected = requested(6);
        let passed = match condition(6) {
            6 => version.suite_mask as u32 & expected == expected,
            7 => expected == 0 || version.suite_mask as u32 & expected != 0,
            _ => return INVALID,
        };
        if !passed {
            return MISMATCH;
        }
    }
    for (bit, value) in [(3, version.platform), (2, version.build)] {
        if mask & (1 << bit) != 0 && !compare(value, requested(bit), condition(bit)) {
            return MISMATCH;
        }
    }
    // Windows compares version/service-pack fields hierarchically. A greater
    // major version satisfies >= without independently checking the minor field.
    let mut last = 0;
    let mut passed = true;
    for (bit, value) in [
        (1, version.major),
        (0, version.minor),
        (5, u32::from(version.service_pack_major)),
        (4, u32::from(version.service_pack_minor)),
    ] {
        if mask & (1 << bit) == 0 {
            continue;
        }
        let predicate = update(&mut last, condition(bit));
        passed = compare(value, requested(bit), predicate);
        if value != requested(bit) || !(1..=5).contains(&predicate) {
            break;
        }
    }
    if passed {
        0
    } else {
        MISMATCH
    }
}
