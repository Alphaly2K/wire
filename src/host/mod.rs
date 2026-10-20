//! Host services receive owned requests and do not interpret guest state.
pub(crate) mod audio;
pub(crate) mod calendar;
pub(crate) mod cpu_time;
#[cfg(target_os = "macos")]
pub(crate) mod font;
pub(crate) mod graphics;
pub(crate) mod raster;
pub mod ui;
