//! PE32 DC services retain the existing bitmap/font-backed Graphics owner.
use super::{Dc, Graphics, STOCK_BITMAP};

impl Graphics {
    pub(crate) fn compatible_dc(&mut self, source: u32) -> Result<u32, u32> {
        if source != 0 {
            self.dc(source)?;
        }
        let handle = self.handle()?;
        self.dcs.insert(handle, Dc::new(None, STOCK_BITMAP));
        Ok(handle)
    }

    pub(crate) fn delete_dc(&mut self, handle: u32) -> Result<u32, u32> {
        if self.dc(handle)?.window.is_some() {
            return Err(6);
        }
        self.dcs.remove(&handle);
        Ok(1)
    }

    pub(crate) fn device_caps(&self, handle: u32, index: u32) -> Result<u32, u32> {
        self.dc(handle)?;
        Ok(match index {
            0 => 0x300,
            2 => 1,
            4 => 271,
            6 => 203,
            8 | 118 => 1024,
            10 | 117 => 768,
            12 => 32,
            14 => 1,
            24 => u32::MAX,
            38 => 0x2081,
            88 | 90 => 96,
            108 => 24,
            _ => 0,
        })
    }
}
