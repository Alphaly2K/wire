//! PE32 formatting adapter; the existing formatter owns all formatting semantics.
use crate::{api::Args, mem::GuestMem, win32::catalog::WindowsPrintfApi};

pub(crate) fn call(
    id: WindowsPrintfApi,
    args: &Args,
    mem: &GuestMem,
    esp: u32,
) -> Result<u32, String> {
    let varargs = if id.list() {
        args[2]
    } else {
        esp.checked_add(12).ok_or("wsprintf stack overflow")?
    };
    crate::wsprintf::format(mem, args[0], args[1], varargs, id.wide())
}
