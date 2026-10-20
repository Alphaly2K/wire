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
                mem.write_int(last_error, 4, error as u64)?;
                Ok(0)
            }
        }
    }
}
