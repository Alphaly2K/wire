//! Fixed logical desktop values and process-owned screen-reader setting.
pub(crate) const DOUBLE_CLICK_MS: u32 = 500;
pub(crate) const DIALOG_BASE_UNITS: u32 = 8 | (16 << 16);

#[derive(Default)]
pub(crate) struct Settings {
    screen_reader: bool,
}

impl Settings {
    pub(crate) fn screen_reader(&self) -> u32 {
        self.screen_reader as u32
    }
    pub(crate) fn set_screen_reader(&mut self, enabled: u32) {
        self.screen_reader = enabled != 0;
    }
}
