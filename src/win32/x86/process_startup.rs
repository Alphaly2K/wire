//! PE32 startup queries borrow only memory and the existing command pointers.
use crate::{api::Args, mem::GuestMem, win32::catalog::StartupApi};

pub(crate) fn call(
    id: StartupApi,
    args: &Args,
    mem: &GuestMem,
    command: [u32; 2],
) -> Result<(u32, &'static str), String> {
    Ok(match id {
        StartupApi::CommandA => (command[0], ""),
        StartupApi::CommandW => (command[1], ""),
        StartupApi::InfoA | StartupApi::InfoW => {
            let mut info = [0u8; 68];
            info[0..4].copy_from_slice(&68u32.to_le_bytes());
            mem.write_bytes(args[0], &info).map_err(|e| e.to_string())?;
            (0, "zeroed STARTUPINFO")
        }
        StartupApi::Feature => (matches!(args[0], 6 | 10) as u32, ""),
    })
}
