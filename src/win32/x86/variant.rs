/*
 * BSTR ownership operations adapted from Hangover's Wine oleaut32/oleaut.c,
 * commit 3ea3a14941b6a122b892c84141ce9662c008081e.
 * Copyright 1999, 2000 Marcus Meissner
 * LGPL-2.1-or-later; see third_party/hangover/README.md.
 */
use crate::{
    api::Args,
    heap::GuestHeap,
    mem::GuestMem,
    win32::{catalog::VariantApi, com::bstr::Bstrings32, text::units32 as units},
};

pub(crate) fn call(
    id: VariantApi,
    a: &Args,
    mem: &mut GuestMem,
    heap: &mut GuestHeap,
    strings: &mut Bstrings32,
    process_heap: u32,
    last_error: &mut u32,
) -> (u32, String) {
    use VariantApi::*;
    let result = (|| -> Result<u32, u32> {
        match id {
            Copy | ChangeType => {
                mem.ptr(a[0], 16, true).map_err(|_| 0x80070057u32)?;
                let mut bytes = [0; 16];
                mem.read_bytes(a[1], &mut bytes)
                    .map_err(|_| 0x80070057u32)?;
                let vt = u16::from_le_bytes(bytes[..2].try_into().unwrap());
                let target = if id == Copy { vt } else { a[3] as u16 };
                let mut allocation = None;
                if vt == 8 && target == 8 {
                    let old = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
                    let pointer = if old == 0 {
                        0
                    } else {
                        let length = mem
                            .read_u32(old.checked_sub(4).ok_or(0x80070057u32)?)
                            .map_err(|_| 0x80070057u32)?
                            / 2;
                        let text = units(mem, old, length)?;
                        strings.allocate(mem, heap, process_heap, &text)?
                    };
                    allocation = Some(pointer);
                    bytes[8..12].copy_from_slice(&pointer.to_le_bytes());
                } else if vt == 0 && target == 8 {
                    let pointer = strings.allocate(mem, heap, process_heap, &[])?;
                    allocation = Some(pointer);
                    bytes = [0; 16];
                    bytes[0] = 8;
                    bytes[8..12].copy_from_slice(&pointer.to_le_bytes());
                } else if vt != target || !matches!(vt, 0..=7 | 10..=14 | 16..=23) {
                    return Err(0x80020005);
                }
                let old_vt = mem.read_u16(a[0]).map_err(|_| 0x80070057u32)?;
                let clear = (|| -> Result<(), u32> {
                    if old_vt == 8 {
                        let old = mem.read_u32(a[0] + 8).map_err(|_| 0x80070057u32)?;
                        if old != 0 {
                            strings
                                .release(heap, process_heap, old)
                                .ok_or(0x80070057u32)?;
                        }
                    } else if !matches!(old_vt, 0..=7 | 10..=14 | 16..=23) && old_vt & 0x4000 == 0 {
                        return Err(0x80020008);
                    }
                    Ok(())
                })();
                if let Err(error) = clear {
                    if let Some(pointer) = allocation {
                        let _ = strings.release(heap, process_heap, pointer);
                    }
                    return Err(error);
                }
                mem.write_bytes(a[0], &bytes).map_err(|_| 0x80070057u32)?;
                Ok(0)
            }
            Initialize => {
                mem.write_bytes(a[0], &[0; 16]).map_err(|_| 0x80070057u32)?;
                Ok(0)
            }
            Clear => {
                mem.ptr(a[0], 16, true).map_err(|_| 0x80070057u32)?;
                let vt = mem.read_u16(a[0]).map_err(|_| 0x80070057u32)?;
                if vt == 8 {
                    let pointer = mem.read_u32(a[0] + 8).map_err(|_| 0x80070057u32)?;
                    if pointer != 0 {
                        strings
                            .release(heap, process_heap, pointer)
                            .ok_or(0x80070057u32)?;
                    }
                } else if !matches!(vt, 0..=7 | 10..=14 | 16..=23) && vt & 0x4000 == 0 {
                    return Err(0x80020008);
                }
                mem.write_u16(a[0], 0).map_err(|_| 0x80070057u32)?;
                Ok(0)
            }
        }
    })();
    match result {
        Ok(value) => (
            value,
            "common process/string/COM allocation contract".into(),
        ),
        Err(error) => {
            *last_error = error;
            (error, format!("common API error {error:#x}"))
        }
    }
}
