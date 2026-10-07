//! Logical display DC ownership and device-capability queries. No GDI drawing backend yet.
use super::{Memory, Runtime64};
pub(in crate::x64::runtime) fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "GetDC" | "CreateCompatibleDC" | "DeleteDC" | "GetPixelFormat" => 1,
        "ReleaseDC" | "GetDeviceCaps" => 2,
        _ => return None,
    })
}
impl Runtime64 {
    pub(in crate::x64::runtime) fn gdi_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        if matches!(name, "GetDC" | "CreateCompatibleDC") {
            let compatible = name == "CreateCompatibleDC";
            let valid = if compatible {
                args[0] == 0 || self.user.dcs.contains_key(&args[0])
            } else {
                args[0] == 0 || self.user.windows.contains_key(&args[0])
            };
            if !valid {
                self.set_error(mem, if compatible { 6 } else { 1400 })?;
                return Ok(0);
            }
            if self.user.dcs.len() >= 4096 || self.user.next_dc >= super::super::TEB + 0x660000 {
                self.set_error(mem, 8)?;
                return Ok(0);
            }
            let handle = self.user.next_dc;
            self.user.next_dc += 16;
            self.user.dcs.insert(
                handle,
                (
                    if compatible { 0 } else { args[0] },
                    self.threads.active,
                    compatible,
                ),
            );
            return Ok(handle);
        }
        let handle = args[if name == "ReleaseDC" { 1 } else { 0 }];
        let Some(&(window, tid, compatible)) = self.user.dcs.get(&handle) else {
            self.set_error(mem, 6)?;
            return Ok(0);
        };
        match name {
            "ReleaseDC" | "DeleteDC" => {
                if compatible != (name == "DeleteDC")
                    || tid != self.threads.active
                    || (name == "ReleaseDC" && window != args[0])
                {
                    self.set_error(mem, 6)?;
                    return Ok(0);
                }
                self.user.dcs.remove(&handle);
                Ok(1)
            }
            "GetPixelFormat" => Ok(0), // No format has been selected on this DC.
            "GetDeviceCaps" => Ok(match args[1] {
                0 => 0x400,
                2 => 1, // DT_RASDISPLAY of the virtual desktop.
                4 => 271,
                6 => 203,
                8 | 118 => 1024,
                10 | 117 => 768,
                12 => 32,
                14 => 1,
                24 => u32::MAX as u64,
                40 | 42 => 36,
                44 => 51,
                88 | 90 => 96,
                108 => 24,
                116 => 60,
                _ => 0, // No unimplemented raster/shading feature is advertised.
            }),
            _ => unreachable!(),
        }
    }
}
