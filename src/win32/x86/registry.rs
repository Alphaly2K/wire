//! PE32 keeps its query-only missing-entry policy, without host registry access.
use crate::{api::Args, mem::GuestMem, win32::catalog::RegistryApi};

pub(crate) fn call(
    id: RegistryApi,
    args: &Args,
    mem: &mut GuestMem,
) -> Result<(u32, String), String> {
    use RegistryApi::*;
    let mut note = String::new();
    let ret = match id {
        OpenA | OpenW | OpenExA | OpenExW => {
            let output = if matches!(id, OpenExA | OpenExW) {
                args[4]
            } else {
                args[2]
            };
            mem.write_u32(output, 0).map_err(|e| e.to_string())?;
            if matches!(args[0], 0x80000000..=0x80000006) {
                let key = crate::file_api::read_name(mem, args[1], id.wide())
                    .map_err(|e| format!("registry key: error {e}"))?;
                note = format!("isolated registry: key {key:?} not present");
                2
            } else {
                6
            }
        }
        QueryExA | QueryExW | QueryA | QueryW => {
            note = "isolated registry: no value / invalid key".into();
            if matches!(args[0], 0x80000000..=0x80000006) {
                2
            } else {
                6
            }
        }
        Close => {
            if matches!(args[0], 0x80000000..=0x80000006) {
                0
            } else {
                6
            }
        }
        _ => unreachable!("ABI-only registry calls handled by the gateway"),
    };
    Ok((ret, note))
}
