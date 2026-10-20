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
