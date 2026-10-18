//! PE32 stock arrow and unclipped signed position, with legacy trace carriers.
use crate::{
    api::Args,
    mem::GuestMem,
    win32::{catalog::CursorApi, user::cursor::Cursor},
};

pub(crate) struct CursorResult {
    pub value: u32,
    pub note: &'static str,
}

pub(crate) fn call(
    id: CursorApi,
    a: &Args,
    mem: &GuestMem,
    cursor: &mut Cursor<u32, u32>,
    tid: u32,
    last_error: &mut u32,
) -> Result<CursorResult, String> {
    use CursorApi::*;
    let note = match id {
        GetPosition => "cursor position",
        SetPosition => "logical cursor position",
        _ => "logical system arrow and per-thread cursor display count",
    };
    let value = match id {
        Show => cursor.show(tid, a[0] != 0) as u32,
        Get => cursor.current(),
        Set => {
            if a[0] != 0 && a[0] != 0x42007f00 {
                *last_error = 6;
                0
            } else {
                cursor.replace(a[0])
            }
        }
        GetPosition => {
            let bytes: Vec<_> = cursor
                .position()
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect();
            mem.write_bytes(a[0], &bytes).map_err(|e| e.to_string())?;
            1
        }
        SetPosition => {
            cursor.set_position([a[0] as i32, a[1] as i32]);
            1
        }
        _ => unreachable!("PE32 clip/pointer/tracking facilities remain ABI-only"),
    };
    Ok(CursorResult { value, note })
}
