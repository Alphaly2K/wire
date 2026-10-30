//! Checked adapters for shared physical key state and the virtual US layout.
use super::{call::Call, catalog::KeyboardApi as K, user, ServiceError};
use crate::{machine::State, memory::Memory, process::Loaded};
#[path = "../../../../src/win32/user/keyboard_layout.rs"]
mod layout;

pub(super) fn dispatch(
    id: K,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    let tid = image.threads.active;
    Ok(match id {
        K::AsyncState => u64::from(image.user.keyboard.asynchronous(a[0] as u32)),
        K::State => {
            let keys = image.user.keyboard.state64(u64::from(tid));
            let key = keys.get(a[0] as usize).copied().unwrap_or(0);
            (((u16::from(key) & 0x80) << 8) | (u16::from(key) & 1)) as i16 as i32 as u32 as u64
        }
        K::StateArray => {
            memory.write(a[0], &image.user.keyboard.state64(u64::from(tid)))?;
            1
        }
        K::Layout => {
            let owner = if a[0] == 0 { tid } else { a[0] as u32 };
            if image.threads.get(owner).is_some_and(|t| t.exit.is_none()) {
                layout::HKL
            } else {
                0
            }
        }
        K::LayoutList => {
            if (a[0] as u32 as i32) < 0 {
                return user::error(memory, state, 87);
            }
            if a[0] != 0 && a[1] != 0 {
                memory.write_int(a[1], memory.mode() as usize / 8, layout::HKL)?;
            }
            1
        }
        K::LayoutNameA | K::LayoutNameW => {
            if id == K::LayoutNameW {
                memory.write(
                    a[0],
                    &"00000409\0"
                        .encode_utf16()
                        .flat_map(u16::to_le_bytes)
                        .collect::<Vec<_>>(),
                )?;
            } else {
                memory.write(a[0], b"00000409\0")?;
            }
            1
        }
        K::Type => match a[0] {
            0 => 4,
            2 => 12,
            _ => 0,
        },
        K::MapA | K::MapW | K::MapExA | K::MapExW => {
            if matches!(id, K::MapExA | K::MapExW) && a[2] != 0 && a[2] != layout::HKL {
                return user::error(memory, state, 87);
            }
            u64::from(layout::map_key(a[0] as u32, a[1] as u32))
        }
        K::Unicode | K::UnicodeEx => {
            if (a[4] as u32 as i32) <= 0 {
                return Ok(0);
            }
            if a[5] & !7 != 0 || id == K::UnicodeEx && a[6] != 0 && a[6] != layout::HKL {
                return user::error(memory, state, 87);
            }
            let mut keys = [0; 256];
            memory.read(a[2], &mut keys)?;
            let text = if a[1] & 0x8000 != 0 && a[5] & 2 == 0 {
                None
            } else {
                layout::character(a[0] as u32, &keys)
            };
            let mut out = text.unwrap_or(0).to_le_bytes().to_vec();
            if a[4] > 1 {
                out.extend_from_slice(&[0, 0]);
            }
            memory.write(a[3], &out)?;
            u64::from(text.is_some())
        }
        K::KeyNameA | K::KeyNameW => {
            if (a[2] as u32 as i32) <= 0 || a[1] == 0 {
                return Ok(0);
            }
            let scan = ((a[0] >> 16) & 255) as u32 | if a[0] & (1 << 24) != 0 { 0xe000 } else { 0 };
            let text = layout::key_name(scan, a[0] & (1 << 25) != 0);
            let count = text.len().min(a[2] as usize - 1);
            let mut out = text.as_bytes()[..count].to_vec();
            out.push(0);
            if id == K::KeyNameW {
                out = out
                    .into_iter()
                    .flat_map(|c| u16::from(c).to_le_bytes())
                    .collect();
            }
            memory.write(a[1], &out)?;
            count as u64
        }
    })
}
