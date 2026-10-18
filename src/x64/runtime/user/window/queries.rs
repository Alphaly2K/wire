//! Win64 scalar queries; callbacks stay in the parent window coordinator.
use super::super::{write_rect, Memory, Runtime64};
use crate::{win32::catalog::WindowQueryApi as Q, x64::memory::WRITE};

impl Runtime64 {
    pub(in crate::x64::runtime) fn window_query_call(
        &mut self,
        mem: &mut Memory,
        id: Q,
        args: &[u64],
    ) -> Result<u64, String> {
        if id.pointer() && id.setting() && args[1] as u32 as i32 == -4 {
            if args[2] == 0 {
                self.set_error(mem, 87)?;
                return Ok(0);
            }
            self.valid_procedure(mem, args[2])?;
        }
        if id == Q::IsWindow {
            return Ok(self.user.windows.contains_key(&args[0]) as u64);
        }
        let Some(w) = self.user.windows.get_mut(&args[0]) else {
            self.set_error(mem, 1400)?;
            return Ok(0);
        };
        match id {
            Q::Parent => Ok(w.parent),
            Q::Visible => Ok((w.style & 0x10000000 != 0) as u64),
            Q::Unicode => Ok(w.wide as u64),
            Q::Iconic => Ok((w.style & 0x20000000 != 0) as u64),
            Q::ClientRect => {
                write_rect(
                    mem,
                    args[1],
                    [0, 0, w.client[2] - w.client[0], w.client[3] - w.client[1]],
                )?;
                Ok(1)
            }
            Q::WindowRect => {
                write_rect(mem, args[1], w.rect)?;
                Ok(1)
            }
            Q::Placement => {
                // WINDOWPLACEMENT uses only DWORD/POINT/RECT fields, 44 bytes.
                if args[1] == 0 || mem.read_int(args[1], 4)? != 44 {
                    self.set_error(mem, 87)?;
                    return Ok(0);
                }
                mem.check(args[1], 44, WRITE)?;
                if w.style & 0x21000000 != 0 {
                    self.set_error(mem, 50)?;
                    return Ok(0);
                }
                let mut bytes = [0; 44];
                bytes[..4].copy_from_slice(&44u32.to_le_bytes());
                bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
                bytes[12..28].fill(0xff);
                for (i, value) in w.rect.iter().enumerate() {
                    bytes[28 + i * 4..32 + i * 4].copy_from_slice(&value.to_le_bytes());
                }
                mem.write(args[1], &bytes)?;
                Ok(1)
            }
            Q::ClientToScreen | Q::ScreenToClient => {
                let mut bytes = [0; 8];
                mem.read(args[1], &mut bytes)?;
                for i in 0..2 {
                    let value = i32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap());
                    let value = if id == Q::ClientToScreen {
                        value.wrapping_add(w.client[i])
                    } else {
                        value.wrapping_sub(w.client[i])
                    };
                    bytes[4 * i..4 * i + 4].copy_from_slice(&value.to_le_bytes());
                }
                mem.write(args[1], &bytes)?;
                Ok(1)
            }
            Q::ThreadProcess => {
                if args[1] != 0 {
                    mem.write_int(args[1], 4, 1)?;
                }
                Ok(w.tid)
            }
            Q::LongA
            | Q::LongW
            | Q::SetLongA
            | Q::SetLongW
            | Q::LongPtrA
            | Q::LongPtrW
            | Q::SetLongPtrA
            | Q::SetLongPtrW => {
                let ptr = id.pointer();
                let width = if ptr { 8 } else { 4 };
                let index = args[1] as u32 as i32;
                let field = match index {
                    -16 => Some(w.style as u64),
                    -20 => Some(w.ex_style as u64),
                    -4 if ptr => Some(w.procedure),
                    -6 if ptr => Some(w.instance),
                    -8 if ptr => Some(w.parent),
                    -12 if ptr => Some(w.menu),
                    -21 if ptr => Some(w.data),
                    _ => None,
                };
                let old = if let Some(field) = field {
                    field
                } else if index >= 0
                    && (index as usize)
                        .checked_add(width)
                        .is_some_and(|end| end <= w.extra.len())
                {
                    let mut bytes = [0; 8];
                    bytes[..width]
                        .copy_from_slice(&w.extra[index as usize..index as usize + width]);
                    u64::from_le_bytes(bytes)
                } else {
                    self.set_error(mem, 1413)?;
                    return Ok(0);
                };
                if id.setting() {
                    let value = args[2];
                    match index {
                        -16 => w.style = value as u32,
                        -20 => w.ex_style = value as u32,
                        -4 => w.procedure = value,
                        -6 => w.instance = value,
                        -8 => return Err("Win64 reparenting is not implemented".into()),
                        -12 => w.menu = value,
                        -21 => w.data = value,
                        _ => w.extra[index as usize..index as usize + width]
                            .copy_from_slice(&value.to_le_bytes()[..width]),
                    }
                }
                Ok(old)
            }
            _ => unreachable!("absent Win64 query"),
        }
    }
}
