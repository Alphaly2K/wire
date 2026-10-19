//! The historical PE32 shell directory shim borrows its shared process heap.
use crate::{api::Args, heap::GuestHeap, mem::GuestMem, win32::catalog::ShellApi};

pub(crate) fn call(
    id: ShellApi,
    a: &Args,
    mem: &mut GuestMem,
    heap: &mut GuestHeap,
    process_heap: u32,
    last_error: &mut u32,
) -> (u32, String) {
    let result = (|| -> Result<u32, u32> {
        match id {
            ShellApi::SpecialFolderA | ShellApi::SpecialFolderW => {
                crate::file_api::write_string(mem, a[1], 260, "C:\\", id.wide())?;
                Ok(1)
            }
            ShellApi::FolderA | ShellApi::FolderW => {
                crate::file_api::write_string(mem, a[4], 260, "C:\\", id.wide())?;
                Ok(0)
            }
            ShellApi::KnownFolder => {
                mem.write_u32(a[3], 0).map_err(|_| 87u32)?;
                let p = heap.alloc(mem, process_heap, 8, false).map_err(|_| 8u32)?;
                if p == 0 {
                    return Err(8);
                }
                crate::file_api::write_string(mem, p, 4, "C:\\", true)?;
                mem.write_u32(a[3], p).map_err(|_| 87u32)?;
                Ok(0)
            }
            ShellApi::Arguments => unreachable!("PE32 CommandLineToArgvW retains ABI-only policy"),
        }
    })();
    match result {
        Ok(v) => (v, "code-page / sandbox directory shim".into()),
        Err(e) => {
            *last_error = e;
            (
                if id.hresult() { 0x8007_0000 | e } else { 0 },
                format!("Win32 error {e}"),
            )
        }
    }
}
