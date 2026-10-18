//! PE32 fixed desktop queries; settings stay ABI-only and DWM stays unknown.
use crate::win32::{catalog::DesktopApi, user::desktop};

pub(crate) fn call(id: DesktopApi) -> (u32, &'static str) {
    match id {
        DesktopApi::DoubleClickTime => (desktop::DOUBLE_CLICK_MS, "input timing profile"),
        DesktopApi::DialogBaseUnits => (
            desktop::DIALOG_BASE_UNITS,
            "initial fixed dialog font units",
        ),
        _ => unreachable!("only fixed desktop values have a PE32 typed binding"),
    }
}
