//! Scratch allocation stays at the PE32 gateway; output ordering stays here.
use crate::{mem::GuestMem, win32::catalog::EnvironmentApi};
pub(crate) fn call(
    id: EnvironmentApi,
    mem: &GuestMem,
    storage: u32,
) -> Result<(u32, &'static str), String> {
    Ok(match id {
        EnvironmentApi::Block => {
            mem.write_u16(storage, 0).map_err(|e| e.to_string())?;
            (storage, "empty environment block")
        }
        EnvironmentApi::BlockW => {
            mem.write_u32(storage, 0).map_err(|e| e.to_string())?;
            (storage, "empty environment block")
        }
        EnvironmentApi::FreeA | EnvironmentApi::FreeW => (1, ""),
        EnvironmentApi::GetA | EnvironmentApi::GetW => (0, "empty environment"),
        _ => unreachable!("ABI-only environment calls use the gateway error 120 policy"),
    })
}
