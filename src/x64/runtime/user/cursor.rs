//! Borrow only cursor state, image identity and window membership at the facade.
use super::{Memory, User};
use crate::win32::{catalog::CursorApi, x64::cursor};

impl User {
    pub(in crate::x64::runtime) fn cursor_call(
        &mut self,
        mem: &mut Memory,
        id: CursorApi,
        a: &[u64],
        tid: u64,
        last_error: u64,
    ) -> Result<u64, String> {
        use CursorApi::*;
        if id == PointerDevices {
            if a[0] == 0 {
                mem.write_int(last_error, 4, 87)?;
                return Ok(0);
            }
            // The virtual desktop has no pen, touch or touchpad digitizers.
            mem.write_int(a[0], 4, 0)?;
            return Ok(1);
        }
        if matches!(
            id,
            EnablePointer | PointerEnabled | PointerType | PointerInfo
        ) {
            let value = match id {
                EnablePointer => {
                    if !self.mouse_pointer.enable(a[0] != 0) {
                        mem.write_int(last_error, 4, 5)?;
                        return Ok(0);
                    }
                    1
                }
                PointerEnabled => u64::from(self.mouse_pointer.enabled()),
                PointerType | PointerInfo => {
                    let Some(snapshot) = self.mouse_pointer.current.get(&tid).filter(|_| a[0] == 1)
                    else {
                        mem.write_int(last_error, 4, 87)?;
                        return Ok(0);
                    };
                    if id == PointerType {
                        mem.write_int(a[1], 4, 4)?;
                    } else {
                        mem.write(a[1], &snapshot.info64())?;
                    }
                    1
                }
                _ => unreachable!(),
            };
            return Ok(value);
        }
        let result = if id == CursorApi::Track {
            cursor::track(mem, a[0], |window| self.windows.contains_key(&window))?
        } else {
            let valid_image = id != CursorApi::Set || a[0] == 0 || self.images.is_cursor(a[0]);
            self.cursor.call(mem, id, a, tid, valid_image)?
        };
        if let Some(error) = result.error {
            mem.write_int(last_error, 4, error as u64)?;
        }
        Ok(result.value)
    }
}
