//! Checked Win32 adapters for the common virtual-memory API. Remote-process
//! access and allocation extensions without a backend fail explicitly.
use crate::api::ApiState;
use crate::mem::GuestMem;

pub fn dispatch(
    dll: &str,
    name: &str,
    a: &crate::api::Args,
    mem: &mut GuestMem,
    state: &mut ApiState,
) -> Option<(u32, String)> {
    if dll != "kernel32.dll"
        || !matches!(
            name,
            "VirtualAlloc"
                | "VirtualAllocEx"
                | "VirtualFree"
                | "VirtualFreeEx"
                | "VirtualProtect"
                | "VirtualProtectEx"
                | "VirtualQuery"
                | "VirtualQueryEx"
        )
    {
        return None;
    }
    let result = (|| -> Result<u32, u32> {
        let args = if name.ends_with("Ex") {
            if a[0] != u32::MAX {
                return Err(6);
            }
            &a[1..]
        } else {
            &a[..]
        };
        match name.trim_end_matches("Ex") {
            "VirtualAlloc" => mem.virtual_alloc(args[0], args[1], args[2], args[3]),
            "VirtualFree" => mem.virtual_free(args[0], args[1], args[2]).map(|_| 1),
            "VirtualProtect" => mem
                .protect_with_output(args[0], args[1], args[2], args[3])
                .map(|_| 1),
            "VirtualQuery" => {
                if args[1] == 0 || args[2] < 28 {
                    return Err(87);
                }
                let info = mem.virtual_query(args[0])?;
                mem.write_bytes(args[1], &info.encode())
                    .map_err(|_| 87u32)?;
                Ok(28)
            }
            _ => unreachable!(),
        }
    })();
    Some(match result {
        Ok(value) => (value, "guest VMM: 4 KiB pages / checked permissions".into()),
        Err(error) => {
            state.last_error = error;
            (0, format!("guest VMM error {error}"))
        }
    })
}
