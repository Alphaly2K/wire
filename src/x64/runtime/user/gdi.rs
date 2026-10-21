//! Typed Win64 DC adapter, borrowing the private logical DC owner.
use super::{Memory, User};
use crate::win32::{catalog::GdiDcApi, gdi::dc::capabilities64};

impl User {
    pub(in crate::x64::runtime) fn gdi_dc_call(
        &mut self,
        mem: &mut Memory,
        id: GdiDcApi,
        args: &[u64],
        tid: u64,
        last_error: u64,
    ) -> Result<u64, String> {
        use GdiDcApi::*;
        let result = match id {
            Window | Compatible => self.dcs.acquire(args[0], tid, id == Compatible, |hwnd| {
                self.windows.contains_key(&hwnd)
            }),
            Release | Delete => self.dcs.release(
                args[if id == Release { 1 } else { 0 }],
                if id == Release { args[0] } else { 0 },
                tid,
                id == Delete,
            ),
            Capabilities | PixelFormat => self.dcs.validate(args[0]).map(|()| {
                if id == Capabilities {
                    match args[1] as u32 {
                        4 => (self.display_size[0] as u64 * 254 + 480) / 960,
                        6 => (self.display_size[1] as u64 * 254 + 480) / 960,
                        8 | 118 => self.display_size[0] as u64,
                        10 | 117 => self.display_size[1] as u64,
                        index => capabilities64(index),
                    }
                } else {
                    0
                }
            }),
            WindowFrame => unreachable!("GetWindowDC has no Win64 binding"),
        };
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                mem.write_int(last_error, 4, error as u64)?;
                Ok(0)
            }
        }
    }
}
