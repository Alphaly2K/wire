//! PE32 messages retain their process-heap allocation and original trim note.
use crate::{api::Args, heap::GuestHeap, mem::GuestMem, win32::catalog::MessageFormatApi};

pub(crate) fn call(
    id: MessageFormatApi,
    args: &Args,
    mem: &mut GuestMem,
    heap: &mut GuestHeap,
    process_heap: u32,
    last_error: &mut u32,
) -> Result<(u32, String), String> {
    let wide = id.wide();
    let text = if args[0] & 0x400 != 0 {
        crate::file_api::read_name(mem, args[1], wide).map_err(|e| format!("FormatMessage: {e}"))?
    } else {
        match args[2] {
            2 => "The system cannot find the file specified.\r\n".into(),
            5 => "Access is denied.\r\n".into(),
            87 => "The parameter is incorrect.\r\n".into(),
            n => format!("Win32 error {n}.\r\n"),
        }
    };
    let count = if wide {
        text.encode_utf16().count()
    } else {
        crate::codepage::encode(&text, 932)
            .map_err(|e| format!("FormatMessage: {e}"))?
            .0
            .len()
    } as u32;
    let ret;
    if args[0] & 0x100 != 0 {
        let p = heap.alloc(
            mem,
            process_heap,
            (count + 1).max(args[5]) * if wide { 2 } else { 1 },
            false,
        )?;
        if p == 0 {
            *last_error = 8;
            ret = 0;
        } else {
            crate::file_api::write_string(mem, p, count + 1, &text, wide)
                .map_err(|e| format!("FormatMessage: {e}"))?;
            mem.write_u32(args[4], p).map_err(|e| e.to_string())?;
            ret = count;
        }
    } else if args[5] <= count {
        *last_error = 122;
        ret = 0;
    } else {
        crate::file_api::write_string(mem, args[4], args[5], &text, wide)
            .map_err(|e| format!("FormatMessage: {e}"))?;
        ret = count;
    }
    Ok((ret, text.trim().into()))
}
