//! Borrow only cursor state, image identity and window membership at the facade.
use super::super::{Memory, Runtime64};
use crate::win32::{catalog::CursorApi, x64::cursor};

impl Runtime64 {
    pub(in crate::x64::runtime) fn cursor_call(
        &mut self,
        mem: &mut Memory,
        id: CursorApi,
        a: &[u64],
    ) -> Result<u64, String> {
        let result = if id == CursorApi::Track {
            cursor::track(mem, a[0], |window| self.user.windows.contains_key(&window))?
        } else {
            let valid_image = id != CursorApi::Set
                || a[0] == 0
                || self
                    .user
                    .images
                    .get(&a[0])
                    .is_some_and(|image| image.kind == 2);
            self.user
                .cursor
                .call(mem, id, a, self.threads.active, valid_image)?
        };
        if let Some(error) = result.error {
            self.set_error(mem, error)?;
        }
        Ok(result.value)
    }
}
