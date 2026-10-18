//! Typed Win64 DC adapter, borrowing the private logical DC owner.
use super::{Memory, Runtime64};
use crate::win32::{catalog::GdiDcApi, gdi::dc::capabilities64};

impl Runtime64 {
    pub(in crate::x64::runtime) fn gdi_dc_call(
        &mut self,
        mem: &mut Memory,
        id: GdiDcApi,
        args: &[u64],
    ) -> Result<u64, String> {
        use GdiDcApi::*;
        let result = match id {
            Window | Compatible => {
                self.user
                    .dcs
                    .acquire(args[0], self.threads.active, id == Compatible, |hwnd| {
                        self.user.windows.contains_key(&hwnd)
                    })
            }
            Release | Delete => self.user.dcs.release(
                args[if id == Release { 1 } else { 0 }],
                if id == Release { args[0] } else { 0 },
                self.threads.active,
                id == Delete,
            ),
            Capabilities | PixelFormat => self.user.dcs.validate(args[0]).map(|()| {
                if id == Capabilities {
                    capabilities64(args[1] as u32)
                } else {
                    0
                }
            }),
            WindowFrame => unreachable!("GetWindowDC has no Win64 binding"),
        };
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                self.set_error(mem, error)?;
                Ok(0)
            }
        }
    }
}
