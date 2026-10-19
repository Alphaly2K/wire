//! Win64 startup layout and feature profile retain their original boundaries.
use crate::{win32::catalog::StartupApi, x64::memory::Memory};

pub(crate) fn call(
    id: StartupApi,
    args: &[u64],
    mem: &mut Memory,
    command: [u64; 2],
) -> Result<u64, String> {
    Ok(match id {
        StartupApi::CommandA => command[0],
        StartupApi::CommandW => command[1],
        StartupApi::InfoA | StartupApi::InfoW => {
            let mut info = [0; 104];
            info[..4].copy_from_slice(&104u32.to_le_bytes());
            mem.write(args[0], &info)?;
            0
        }
        StartupApi::Feature => (args[0] as u32 == 12) as u64,
    })
}
