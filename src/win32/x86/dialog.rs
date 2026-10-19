//! Existing diagnostic-only MessageBox policy; the text is recorded in API trace.
use crate::{api::read_str, mem::GuestMem, win32::catalog::DialogApi};

pub(crate) fn message_box(id: DialogApi, args: &[u32], mem: &GuestMem) -> (u32, String) {
    debug_assert!(id.message_box());
    let wide = id == DialogApi::MessageBoxW;
    let text = read_str(mem, args[1], wide);
    let caption = read_str(mem, args[2], wide);
    (1, format!("SHOWN: [{caption}] {text}")) // IDOK; no host dialog.
}
