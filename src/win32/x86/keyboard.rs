//! PE32 key-state calls borrow the existing keyboard owner and current tid.
use crate::{api::Args, keyboard::Keyboard, mem::GuestMem, win32::catalog::KeyboardApi};

pub(crate) fn call(
    id: KeyboardApi,
    args: &Args,
    mem: &mut GuestMem,
    keyboard: &mut Keyboard,
    tid: u32,
) -> Result<u32, String> {
    Ok(match id {
        KeyboardApi::AsyncState => keyboard.asynchronous(args[0]),
        KeyboardApi::State => {
            let value = keyboard
                .state(tid)
                .get(args[0] as usize)
                .copied()
                .unwrap_or(0) as u16;
            (((value & 0x80) << 8) | (value & 1)) as i16 as i32 as u32
        }
        KeyboardApi::StateArray => {
            mem.write_bytes(args[0], &keyboard.state(tid))
                .map_err(|e| e.to_string())?;
            1
        }
        _ => unreachable!("PE32 keyboard layout helpers remain ABI-only"),
    })
}
