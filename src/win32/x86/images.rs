//! PE32 stock-arrow loading and the explicit historical DestroyIcon rejection.
use crate::{api::Args, win32::catalog::ImageApi};

pub(crate) struct ImageResult {
    pub value: u32,
    pub note: &'static str,
    pub implemented: bool,
}

pub(crate) fn call(id: ImageApi, a: &Args, last_error: &mut u32) -> ImageResult {
    if id == ImageApi::DestroyIcon {
        *last_error = 120;
        return ImageResult {
            value: 0,
            note: "USER32 error 120",
            implemented: false,
        };
    }
    assert!(matches!(id, ImageApi::LoadCursorA | ImageApi::LoadCursorW));
    let value = if a[0] == 0 && a[1] == 32512 {
        0x42007f00
    } else {
        *last_error = 1812;
        0
    };
    ImageResult {
        value,
        note: "logical system arrow and per-thread cursor display count",
        implemented: true,
    }
}
