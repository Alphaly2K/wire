//! Explicit virtual US keyboard, independent of the guest's Japanese NLS profile.
//! No IME, dead keys or layout switching are advertised. Host-resolved WM_CHAR
//! text continues through TranslateMessage; these APIs use caller-supplied state.
// Scan/character/key-name rules adapted from Hangover's pinned Wine win32u/input.c.
// Copyright 1993 Bob Amstadt, David Metcalfe; 1996 Albrecht Kleine,
// Frans van Dorsselaer; 1997 David Faure; 1998 Morten Welinder, Ulrich Weigand;
// 2001 Eric Pouech; 2002 Alexandre Julliard. LGPL-2.1-or-later; wire selects
// LGPL 2.1. Original notices and source snapshot: third_party/hangover/input.c.

pub(crate) const HKL: u64 = 0x04090409;

// Set-1 scan codes for the virtual keyboard. Navigation VKs share the keypad
// scans; MapVirtualKey is independent of current NumLock/key state.
fn scan_to_vk(scan: u32) -> u32 {
    let row: &[u8] = match scan {
        0x02..=0x0b => b"1234567890",
        0x10..=0x19 => b"QWERTYUIOP",
        0x1e..=0x26 => b"ASDFGHJKL",
        0x2c..=0x32 => b"ZXCVBNM",
        _ => &[],
    };
    if !row.is_empty() {
        let start = match scan {
            0x02..=0x0b => 0x02,
            0x10..=0x19 => 0x10,
            0x1e..=0x26 => 0x1e,
            _ => 0x2c,
        };
        return row[(scan - start) as usize] as u32;
    }
    match scan {
        0x01 => 0x1b,
        0x0c => 0xbd,
        0x0d => 0xbb,
        0x0e => 8,
        0x0f => 9,
        0x1a => 0xdb,
        0x1b => 0xdd,
        0x1c | 0xe01c => 13,
        0x1d => 0xa2,
        0x27 => 0xba,
        0x28 => 0xde,
        0x29 => 0xc0,
        0x2a => 0xa0,
        0x2b => 0xdc,
        0x33 => 0xbc,
        0x34 => 0xbe,
        0x35 => 0xbf,
        0x36 => 0xa1,
        0x37 => 0x6a,
        0x38 => 0xa4,
        0x39 => 0x20,
        0x3a => 0x14,
        0x3b..=0x44 => 0x70 + scan - 0x3b,
        0x45 => 0x90,
        0x46 => 0x91,
        0x47 | 0xe047 => 0x24,
        0x48 | 0xe048 => 0x26,
        0x49 | 0xe049 => 0x21,
        0x4a => 0x6d,
        0x4b | 0xe04b => 0x25,
        0x4c => 0x0c,
        0x4d | 0xe04d => 0x27,
        0x4e => 0x6b,
        0x4f | 0xe04f => 0x23,
        0x50 | 0xe050 => 0x28,
        0x51 | 0xe051 => 0x22,
        0x52 | 0xe052 => 0x2d,
        0x53 | 0xe053 => 0x2e,
        0x54 => 0x2c,
        0x56 => 0xe2,
        0x57 => 0x7a,
        0x58 => 0x7b,
        0xe01d => 0xa3,
        0xe035 => 0x6f,
        0xe037 => 0x2c,
        0xe038 => 0xa5,
        0xe046 => 3,
        0xe05b => 0x5b,
        0xe05c => 0x5c,
        0xe05d => 0x5d,
        0xe11d => 0x13,
        _ => 0,
    }
}
fn vk_to_scan(vk: u32) -> u32 {
    let vk = match vk {
        0x10 => 0xa0,
        0x11 => 0xa2,
        0x12 => 0xa4,
        0x60..=0x69 => {
            [0x2d, 0x23, 0x28, 0x22, 0x25, 0x0c, 0x27, 0x24, 0x26, 0x21][(vk - 0x60) as usize]
        }
        0x6e => 0x2e,
        _ => vk,
    };
    if vk == 0 || vk > 255 {
        return 0;
    }
    // Prefer ordinary scans, then E0/E1, preserving modifier sides in EX mode.
    (1..=0x7f)
        .chain((1..=0x7f).map(|s| s | 0xe000))
        .chain([0xe11d])
        .find(|&s| scan_to_vk(s) == vk)
        .unwrap_or(0)
}
pub(crate) fn character(vk: u32, state: &[u8; 256]) -> Option<u16> {
    if vk > 255 {
        return None;
    }
    let shift = state[0x10] & 0x80 != 0;
    let ctrl = state[0x11] & 0x80 != 0;
    let alt = state[0x12] & 0x80 != 0;
    if ctrl && alt {
        return None; // This layout has no AltGr.
    }
    if ctrl {
        return match vk {
            0x41..=0x5a => Some((vk - 0x40) as u16),
            8 => Some(127),
            13 => Some(10),
            0x1b | 0xdb => Some(27),
            0xdc | 0xe2 => Some(28),
            0xdd => Some(29),
            0x20 => Some(32),
            3 => Some(3),
            0x32 if shift => Some(0),
            0x36 if shift => Some(30),
            0xbd if shift => Some(31),
            _ => None,
        };
    }
    if (0x41..=0x5a).contains(&vk) {
        return Some(
            (vk + if shift ^ (state[0x14] & 1 != 0) {
                0
            } else {
                32
            }) as u16,
        );
    }
    if (0x30..=0x39).contains(&vk) {
        return Some(if shift {
            b")!@#$%^&*("[(vk - 0x30) as usize] as u16
        } else {
            vk as u16
        });
    }
    if (0x60..=0x69).contains(&vk) {
        return (!shift && !alt).then_some((vk - 0x60 + 0x30) as u16);
    }
    let pair = match vk {
        3 => [3, 3],
        8 => [8, 8],
        9 => [9, 9],
        13 => [13, 13],
        0x1b => [27, 27],
        0x20 => [32, 32],
        0x6a => *b"**",
        0x6b => *b"++",
        0x6d => *b"--",
        0x6e if !shift && !alt => *b"..",
        0x6f => *b"//",
        0xba => *b";:",
        0xbb => *b"=+",
        0xbc => *b",<",
        0xbd => *b"-_",
        0xbe => *b".>",
        0xbf => *b"/?",
        0xc0 => *b"`~",
        0xdb => *b"[{",
        0xdc | 0xe2 => *b"\\|",
        0xdd => *b"]}",
        0xde => *b"'\"",
        _ => return None,
    };
    Some(pair[shift as usize] as u16)
}
pub(crate) fn map_key(code: u32, kind: u32) -> u32 {
    match kind {
        0 | 4 => {
            let scan = vk_to_scan(code);
            if kind == 4 {
                scan
            } else if scan & 0xff00 == 0xe100 {
                0
            } else {
                scan & 255
            }
        }
        1 | 3 => {
            let vk = scan_to_vk(code);
            if kind == 3 {
                vk
            } else {
                match vk {
                    0xa0 | 0xa1 => 0x10,
                    0xa2 | 0xa3 => 0x11,
                    0xa4 | 0xa5 => 0x12,
                    _ => vk,
                }
            }
        }
        2 if (0x41..=0x5a).contains(&code) => code,
        2 => character(code, &[0; 256]).unwrap_or(0) as u32,
        _ => 0,
    }
}
pub(crate) fn key_name(mut scan: u32, dont_care: bool) -> String {
    if dont_care {
        scan = match scan {
            0x36 => 0x2a,
            0xe01d => 0x1d,
            0xe038 => 0x38,
            _ => scan,
        };
    }
    let name = match scan {
        0x01 => "Esc",
        0x0e => "Backspace",
        0x0f => "Tab",
        0x1c => "Enter",
        0x1d => "Ctrl",
        0x2a => "Shift",
        0x36 => "Right Shift",
        0x37 => "Num *",
        0x38 => "Alt",
        0x39 => "Space",
        0x3a => "Caps Lock",
        0x45 => "Pause",
        0x46 => "Scroll Lock",
        0x47 => "Num 7",
        0x48 => "Num 8",
        0x49 => "Num 9",
        0x4a => "Num -",
        0x4b => "Num 4",
        0x4c => "Num 5",
        0x4d => "Num 6",
        0x4e => "Num +",
        0x4f => "Num 1",
        0x50 => "Num 2",
        0x51 => "Num 3",
        0x52 => "Num 0",
        0x53 => "Num Del",
        0x54 => "Sys Req",
        0xe01c => "Num Enter",
        0xe01d => "Right Ctrl",
        0xe035 => "Num /",
        0xe037 => "Prnt Scrn",
        0xe038 => "Right Alt",
        0xe045 => "Num Lock",
        0xe046 => "Break",
        0xe047 => "Home",
        0xe048 => "Up",
        0xe049 => "Page Up",
        0xe04b => "Left",
        0xe04d => "Right",
        0xe04f => "End",
        0xe050 => "Down",
        0xe051 => "Page Down",
        0xe052 => "Insert",
        0xe053 => "Delete",
        0xe05b => "Left Windows",
        0xe05c => "Right Windows",
        0xe05d => "Application",
        0x3b..=0x44 => return format!("F{}", scan - 0x3b + 1),
        0x57 | 0x58 => return format!("F{}", scan - 0x57 + 11),
        _ => {
            return char::from_u32(map_key(scan_to_vk(scan), 2))
                .filter(|&c| c != '\0')
                .map(|c| c.to_string())
                .unwrap_or_default()
        }
    };
    name.to_owned()
}
