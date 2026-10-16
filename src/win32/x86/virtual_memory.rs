//! Checked Win32 adapters for the common virtual-memory API. Remote-process
//! access and allocation extensions without a backend fail explicitly.
use crate::mem::GuestMem;
use crate::win32::catalog::VirtualMemoryApi;

pub(crate) fn call(
    id: VirtualMemoryApi,
    a: &crate::api::Args,
    mem: &mut GuestMem,
    last_error: &mut u32,
) -> (u32, String) {
    let result = (|| -> Result<u32, u32> {
        let args = if id.extended() {
            if a[0] != u32::MAX {
                return Err(6);
            }
            &a[1..]
        } else {
            &a[..]
        };
        match id.base() {
            VirtualMemoryApi::Alloc => mem.virtual_alloc(args[0], args[1], args[2], args[3]),
            VirtualMemoryApi::Free => mem.virtual_free(args[0], args[1], args[2]).map(|_| 1),
            VirtualMemoryApi::Protect => mem
                .protect_with_output(args[0], args[1], args[2], args[3])
                .map(|_| 1),
            VirtualMemoryApi::Query => {
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
    match result {
        Ok(value) => (value, "guest VMM: 4 KiB pages / checked permissions".into()),
        Err(error) => {
            *last_error = error;
            (0, format!("guest VMM error {error}"))
        }
    }
}
