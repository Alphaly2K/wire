//! The current desktop has no installed IME or issued input context.
use crate::win32::catalog::ImeApi;

pub(crate) struct InputResult {
    pub value: u64,
    pub error: Option<u32>,
}
impl InputResult {
    pub fn value(value: u64) -> Self {
        Self { value, error: None }
    }
    pub fn error(value: u64, code: u32) -> Self {
        Self {
            value,
            error: Some(code),
        }
    }
}

/// No payload/output access: all composition contexts remain invalid.
pub(crate) fn ime(id: ImeApi, a: &[u64], window_exists: bool) -> InputResult {
    use ImeApi::*;
    if id == IsIme {
        return InputResult::value(0);
    }
    if id == DefaultWindow {
        return if a[0] != 0 && !window_exists {
            InputResult::error(0, 1400)
        } else {
            InputResult::value(0)
        };
    }
    if matches!(id, GetContext | ReleaseContext | Associate | AssociateEx) && !window_exists {
        return InputResult::error(0, 1400);
    }
    if id == GetContext || (id == Associate && a[1] == 0) {
        return InputResult::value(0);
    }
    if id == AssociateEx && a[1] == 0 && matches!(a[2], 0 | 1) {
        return InputResult::value(1);
    }
    InputResult::error(0, 6)
}
