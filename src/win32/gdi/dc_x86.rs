//! PE32 DC calls borrow the real Graphics backing and window-size lookup.
use crate::{api::Args, graphics::Graphics, win32::catalog::GdiDcApi};

pub(crate) fn call(
    id: GdiDcApi,
    a: &Args,
    graphics: &mut Graphics,
    window_size: impl FnOnce(u32) -> Option<[i32; 2]>,
    last_error: &mut u32,
) -> (u32, String) {
    use GdiDcApi::*;
    let result = match id {
        Window => (|| {
            let [width, height] = if a[0] == 0 {
                [1024, 768]
            } else {
                window_size(a[0]).ok_or(1400u32)?
            };
            graphics.window_dc(a[0], width.max(1) as u32, height.max(1) as u32)
        })(),
        Release => graphics.release_window_dc(a[0], a[1]).map(|()| 1),
        Compatible => graphics.compatible_dc(a[0]),
        Delete => graphics.delete_dc(a[0]),
        Capabilities => graphics.device_caps(a[0], a[1]),
        WindowFrame | PixelFormat => {
            unreachable!("PE32 ABI-only DC calls do not enter a typed adapter")
        }
    };
    match result {
        Ok(value) => (value, "checked GDI object / CPU pixels".into()),
        Err(error) => {
            *last_error = error;
            (0, format!("GDI error {error}"))
        }
    }
}
