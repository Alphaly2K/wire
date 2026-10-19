//! Win64 logical desktop settings and DWM copy-out; process frame is saved first.
use super::{write_rect, Memory, Runtime64};
use crate::win32::catalog::DesktopApi;

impl Runtime64 {
    pub(in crate::x64::runtime) fn desktop_call(
        &mut self,
        mem: &mut Memory,
        id: DesktopApi,
        args: &[u64],
    ) -> Result<u64, String> {
        use DesktopApi::*;
        match id {
            ParametersA | ParametersW => match args[0] {
                0x46 => {
                    mem.write_int(args[2], 4, self.user.desktop.screen_reader() as u64)?;
                    Ok(1)
                }
                0x47 if args[3] == 0 => {
                    self.user.desktop.set_screen_reader(args[1] as u32);
                    Ok(1)
                }
                _ => {
                    self.set_error(mem, 50)?;
                    Ok(0)
                }
            },
            CompositionEnabled => {
                if args[0] == 0 {
                    return Ok(0x80070057);
                }
                mem.write_int(args[0], 4, 0)?;
                Ok(0)
            }
            WindowAttribute => {
                let Some(window) = self.user.windows.get(&args[0]) else {
                    return Ok(0x80070006);
                };
                if args[2] == 0 {
                    return Ok(0x80070057);
                }
                match (args[1], args[3]) {
                    (9, 16) => {
                        write_rect(mem, args[2], window.rect)?;
                        Ok(0)
                    }
                    (14, 4) => {
                        mem.write_int(args[2], 4, 0)?;
                        Ok(0)
                    }
                    _ => Ok(0x80070057),
                }
            }
            DoubleClickTime | DialogBaseUnits | ProcessStation | ObjectInformationW => {
                unreachable!("fixed desktop queries have no Win64 binding")
            }
        }
    }
}
