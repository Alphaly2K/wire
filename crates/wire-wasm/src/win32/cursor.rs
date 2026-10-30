use super::{call::Call, catalog::CursorApi as C, display, user, ServiceError};
use crate::{
    machine::{Mode, State},
    memory::Memory,
    process::Loaded,
};

pub(super) fn dispatch(
    id: C,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable("cursor requires a loaded image"))?;
    let tid = u64::from(image.threads.active);
    let u = &mut image.user;
    let a = &call.arguments;
    Ok(match id {
        C::Show => u.cursor.show(tid, a[0] as u32 != 0) as u32 as u64,
        C::Get => u.cursor.current(),
        C::Set => {
            if a[0] != 0 && !u.images.is_cursor(a[0]) {
                return user::error(memory, state, 6);
            }
            u.cursor.replace(a[0])
        }
        C::GetPosition => {
            memory.write(
                a[0],
                &u.cursor
                    .position()
                    .into_iter()
                    .flat_map(i32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )?;
            1
        }
        C::SetPosition => {
            let point = [a[0] as u32 as i32, a[1] as u32 as i32];
            u.cursor.set_position(
                u.clip
                    .map_or(point, |clip| user::cursor::clamp(point, clip)),
            );
            1
        }
        C::GetClip => {
            user::write_rect(memory, a[0], u.clip.unwrap_or_else(display::bounds))?;
            1
        }
        C::Clip => {
            let clip = if a[0] == 0 {
                None
            } else {
                let rect = user::rect(memory, a[0])?;
                let Some(clip) = user::cursor::clipped(rect, display::bounds()) else {
                    return user::error(memory, state, 87);
                };
                Some(clip)
            };
            if let Some(clip) = clip {
                u.cursor
                    .set_position(user::cursor::clamp(u.cursor.position(), clip));
            }
            u.clip = clip;
            1
        }
        C::EnablePointer => {
            if !u.mouse_pointer.enable(a[0] as u32 != 0) {
                return user::error(memory, state, 5);
            }
            1
        }
        C::PointerEnabled => u64::from(u.mouse_pointer.enabled()),
        C::PointerDevices => {
            if a[0] == 0 {
                return user::error(memory, state, 87);
            }
            memory.write_int(a[0], 4, 0)?;
            1
        }
        C::PointerType | C::PointerInfo => {
            let Some(pointer) = u
                .mouse_pointer
                .current
                .get(&tid)
                .filter(|_| a[0] as u32 == 1)
            else {
                return user::error(memory, state, 87);
            };
            if id == C::PointerType {
                memory.write_int(a[1], 4, 4)?;
            } else if memory.mode() == Mode::X86 {
                memory.write(a[1], &pointer.info32())?;
            } else {
                memory.write(a[1], &pointer.info64())?;
            }
            1
        }
        C::Track => {
            let width = memory.mode() as usize / 8;
            let size = if width == 4 { 16 } else { 24 };
            let mut bytes = [0; 24];
            memory.read(a[0], &mut bytes[..size])?;
            let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            if word(0) != size as u32 {
                return user::error(memory, state, 87);
            }
            let flags = word(4);
            if flags & 0x40000000 != 0 {
                let mut output = [0; 24];
                output[..4].copy_from_slice(&(size as u32).to_le_bytes());
                memory.write(a[0], &output[..size])?;
                return Ok(1);
            }
            let window = if width == 4 {
                u64::from(word(8))
            } else {
                u64::from_le_bytes(bytes[8..16].try_into().unwrap())
            };
            if !u.windows.contains_key(&window) {
                return user::error(memory, state, 1400);
            }
            if flags & !(0x80000000 | 0x10 | 3) != 0 {
                return user::error(memory, state, 87);
            }
            if flags & 0x80000000 == 0 && flags & 3 != 0 {
                return user::error(memory, state, 50);
            }
            1
        }
    })
}
