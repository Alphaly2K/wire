//! Compatibility path for the public native presenter. Guest polling belongs
//! to the architecture adapter; native FFI and owned requests live in host::ui.
pub use crate::host::ui::Presenter;
