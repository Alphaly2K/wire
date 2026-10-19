//! PE32 CRT byte, string and clock helpers; no process or CPU state.
use crate::{api::Args, mem::GuestMem};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn call(name: &str, args: &Args, mem: &GuestMem) -> Result<Option<u32>, String> {
    let value = match name {
        "time" | "_time32" => {
            let n = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs();
            let value = if n > i32::MAX as u64 {
                u32::MAX
            } else {
                n as u32
            };
            if args[0] != 0 {
                mem.write_u32(args[0], value).map_err(|e| e.to_string())?;
            }
            value
        }
        "_mbsinc" => {
            let first = mem.read_u8(args[0]).map_err(|e| e.to_string())?;
            let lead = matches!(first, 0x81..=0x9f | 0xe0..=0xfc);
            let pair = lead
                && mem
                    .read_u8(args[0].checked_add(1).ok_or("_mbsinc address overflow")?)
                    .map_err(|e| e.to_string())?
                    != 0;
            args[0]
                .checked_add(if pair { 2 } else { 1 })
                .ok_or("_mbsinc address overflow")?
        }
        "_mbsnbcpy" => {
            // Hangover Wine mbcs.c, LGPL-2.1-or-later, Alexandre Julliard
            // (1999), Jon Griffths (2000). Checked CP932 guest adaptation.
            if args[2] == 0 {
                return Ok(Some(args[0]));
            }
            if args[2] > 1 << 26 {
                return Err("_mbsnbcpy size too large".into());
            }
            let mut bytes = vec![0; args[2] as usize];
            let mut lead = false;
            let mut copied = 0;
            for i in 0..args[2] {
                let b = mem
                    .read_u8(args[1].checked_add(i).ok_or("_mbsnbcpy address overflow")?)
                    .map_err(|e| e.to_string())?;
                if b == 0 {
                    break;
                }
                lead = !lead && matches!(b, 0x81..=0x9f | 0xe0..=0xfc);
                bytes[i as usize] = b;
                copied += 1;
            }
            if lead {
                bytes[copied - 1] = 0;
            }
            mem.write_bytes(args[0], &bytes)
                .map_err(|e| e.to_string())?;
            args[0]
        }
        "memcpy" | "memmove" => {
            let n = (args[2] as usize).min(1 << 26);
            let mut tmp = vec![0u8; n];
            mem.read_bytes(args[1], &mut tmp)
                .map_err(|e| e.to_string())?;
            mem.write_bytes(args[0], &tmp).map_err(|e| e.to_string())?;
            args[0]
        }
        "memset" => {
            let n = (args[2] as usize).min(1 << 26);
            let tmp = vec![args[1] as u8; n];
            mem.write_bytes(args[0], &tmp).map_err(|e| e.to_string())?;
            args[0]
        }
        "memcmp" => {
            if args[2] == 0 {
                0
            } else {
                if args[2] > 1 << 26 {
                    return Err("memcmp range too large".into());
                }
                let left = mem
                    .ptr(args[0], args[2], false)
                    .map_err(|e| e.to_string())?;
                let right = mem
                    .ptr(args[1], args[2], false)
                    .map_err(|e| e.to_string())?;
                // Both complete ranges are checked, and no guest callback or
                // mutation can invalidate the backing during comparison.
                let (left, right) = unsafe {
                    (
                        std::slice::from_raw_parts(left, args[2] as usize),
                        std::slice::from_raw_parts(right, args[2] as usize),
                    )
                };
                left.iter()
                    .zip(right)
                    .find_map(|(&a, &b)| (a != b).then_some((a as i32 - b as i32) as u32))
                    .unwrap_or(0)
            }
        }
        "toupper" | "tolower" => {
            // The current CRT supports the default C locale. Values outside
            // ASCII (including EOF) retain their exact integer value.
            match (name, args[0]) {
                ("toupper", 0x61..=0x7a) => args[0] - 32,
                ("tolower", 0x41..=0x5a) => args[0] + 32,
                _ => args[0],
            }
        }
        "strcmp" | "strncmp" => {
            let limit = if name == "strncmp" { args[2] } else { u32::MAX };
            let mut result = 0;
            for i in 0..limit {
                let left = mem
                    .read_u8(args[0].checked_add(i).ok_or("strcmp address overflow")?)
                    .map_err(|e| e.to_string())?;
                let right = mem
                    .read_u8(args[1].checked_add(i).ok_or("strcmp address overflow")?)
                    .map_err(|e| e.to_string())?;
                if left != right {
                    result = (left as i32 - right as i32) as u32;
                    break;
                }
                if left == 0 {
                    break;
                }
            }
            result
        }
        "strlen" => {
            let mut len = None;
            for i in 0..1 << 20 {
                if mem
                    .read_u8(args[0].checked_add(i).ok_or("strlen address overflow")?)
                    .map_err(|e| e.to_string())?
                    == 0
                {
                    len = Some(i);
                    break;
                }
            }
            len.ok_or("unterminated strlen source")?
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
