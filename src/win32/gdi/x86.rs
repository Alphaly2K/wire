//! GDI/USER ABI routing, window dimensions, LastError and diagnostics.
use crate::{
    api::{ApiState, Args},
    mem::GuestMem,
};
pub(crate) fn dispatch(
    dll: &str,
    name: &str,
    a: &Args,
    mem: &mut GuestMem,
    state: &mut ApiState,
) -> Option<(u32, String, bool)> {
    let gdi = dll == "gdi32.dll"
        && matches!(
            name,
            "CreateCompatibleBitmap"
                | "CreateDIBSection"
                | "DeleteObject"
                | "SelectObject"
                | "GetObjectA"
                | "GetObjectW"
                | "GetObjectType"
                | "GetCurrentObject"
                | "GetStockObject"
                | "SetPixel"
                | "SetPixelV"
                | "GetPixel"
                | "BitBlt"
                | "SetWindowOrgEx"
                | "SetViewportOrgEx"
                | "GetWindowOrgEx"
                | "GetViewportOrgEx"
                | "SetTextColor"
                | "GetTextColor"
                | "SetBkColor"
                | "GetBkColor"
                | "GdiFlush"
                | "StretchDIBits"
                | "GetDIBits"
                | "CreateFontA"
                | "CreateFontW"
                | "CreateFontIndirectA"
                | "CreateFontIndirectW"
                | "GetTextMetricsW"
                | "SetMapMode"
                | "GetMapMode"
        );
    if !gdi {
        return None;
    }
    let result = crate::graphics::execute_gdi(name, a, mem, &mut state.graphics);
    let info = if name == "CreateDIBSection" {
        let mut bytes = [0; 40];
        if mem.read_bytes(a[1], &mut bytes).is_ok() {
            format!(
                "; BITMAPINFO={:x?}",
                bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u32::from_le_bytes(*b))
                    .collect::<Vec<_>>()
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    Some(match result {
        Ok(v) => (v, "checked GDI object / CPU pixels".into(), true),
        Err(e) => {
            state.last_error = e;
            let v = if matches!(
                name,
                "GetPixel"
                    | "SetPixel"
                    | "SetTextColor"
                    | "GetTextColor"
                    | "SetBkColor"
                    | "GetBkColor"
            ) {
                u32::MAX
            } else {
                0
            };
            (v, format!("GDI error {e}{info}"), e != 120)
        }
    })
}
