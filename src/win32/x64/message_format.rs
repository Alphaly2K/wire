//! Checked message preparation; allocation and LocalFree registration stay with the runtime.
use crate::{
    win32::catalog::MessageFormatApi,
    x64::memory::{Memory, WRITE},
};

pub(crate) enum Prepared {
    Done(u64),
    Allocate {
        bytes: Vec<u8>,
        count: u64,
        size: u64,
    },
}

pub(crate) fn prepare(
    mem: &mut Memory,
    id: MessageFormatApi,
    args: &[u64],
    error_slot: u64,
) -> Result<Prepared, String> {
    let flags = args[0] as u32;
    if flags & !0x37ff != 0 || flags & 0x1400 == 0 || flags & 0x1400 == 0x1400 {
        mem.write_int(error_slot, 4, 87)?;
        return Ok(Prepared::Done(0));
    }
    if flags & 0xff != 0 || flags & 0x800 != 0 || !matches!(args[3], 0 | 0x400 | 0x409) {
        mem.write_int(error_slot, 4, 50)?;
        return Ok(Prepared::Done(0));
    }
    let wide = id.wide();
    let text =
        if flags & 0x400 != 0 {
            super::text::path_name(mem, args[1], wide)?
        } else {
            let message = match args[2] {
            0 => "The operation completed successfully.",
            2 => "The system cannot find the file specified.",
            3 => "The system cannot find the path specified.",
            5 => "Access is denied.",
            6 => "The handle is invalid.",
            8 => "Not enough memory resources are available to process this command.",
            32 => "The process cannot access the file because it is being used by another process.",
            50 => "The request is not supported.",
            87 => "The parameter is incorrect.",
            122 => "The data area passed to a system call is too small.",
            126 => "The specified module could not be found.",
            127 => "The specified procedure could not be found.",
            183 => "Cannot create a file when that file already exists.",
            203 => "The system could not find the environment option that was entered.",
            _ => { mem.write_int(error_slot, 4, 317)?; return Ok(Prepared::Done(0)); }
        };
            format!("{message}\r\n")
        };
    if flags & 0x200 == 0 && (args[6] != 0 || text.contains('%')) {
        mem.write_int(error_slot, 4, 50)?;
        return Ok(Prepared::Done(0));
    }
    let bytes: Vec<u8> = if wide {
        text.encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    } else {
        crate::codepage::encode(&(text + "\0"), 932)
            .map_err(|e| format!("message codepage error {e}"))?
            .0
    };
    let width = if wide { 2 } else { 1 };
    let count = bytes.len() / width - 1;
    if flags & 0x100 != 0 {
        mem.check(args[4], 8, WRITE)?;
        let size = (count as u64 + 1).max(args[5]) * width as u64;
        Ok(Prepared::Allocate {
            bytes,
            count: count as u64,
            size,
        })
    } else if args[5] <= count as u64 {
        mem.write_int(error_slot, 4, 122)?;
        Ok(Prepared::Done(0))
    } else {
        mem.write(args[4], &bytes)?;
        Ok(Prepared::Done(count as u64))
    }
}
