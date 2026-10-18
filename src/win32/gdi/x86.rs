//! PE32 GDI errors/diagnostics; only Graphics and the error slot are borrowed.
use crate::{
    api::Args,
    graphics::{GdiOperation as Op, Graphics},
    mem::GuestMem,
};

pub(crate) fn call(
    op: Op,
    a: &Args,
    mem: &mut GuestMem,
    graphics: &mut Graphics,
    last_error: &mut u32,
) -> (u32, String, bool) {
    let result = crate::graphics::execute_gdi(op, a, mem, graphics);
    let info = if op == Op::DibSection {
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
    match result {
        Ok(v) => (v, "checked GDI object / CPU pixels".into(), true),
        Err(e) => {
            *last_error = e;
            let v = if matches!(
                op,
                Op::GetPixel
                    | Op::SetPixel
                    | Op::SetTextColor
                    | Op::GetTextColor
                    | Op::SetBackgroundColor
                    | Op::GetBackgroundColor
            ) {
                u32::MAX
            } else {
                0
            };
            (v, format!("GDI error {e}{info}"), e != 120)
        }
    }
}
