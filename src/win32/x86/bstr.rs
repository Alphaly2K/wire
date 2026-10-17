/*
 * BSTR operations adapted from Hangover's Wine oleaut32/oleaut.c,
 * commit 3ea3a14941b6a122b892c84141ce9662c008081e.
 * Copyright 1999, 2000 Marcus Meissner
 * LGPL-2.1-or-later; see third_party/hangover/README.md.
 */
use crate::{
    api::Args,
    heap::GuestHeap,
    mem::GuestMem,
    win32::{catalog::BstrApi, com::bstr::Bstrings32, text::units32 as units},
};

pub(crate) fn call(
    id: BstrApi,
    a: &Args,
    mem: &mut GuestMem,
    heap: &mut GuestHeap,
    strings: &mut Bstrings32,
    process_heap: u32,
    last_error: &mut u32,
) -> (u32, String) {
    use BstrApi::*;
    let result = (|| -> Result<u32, u32> {
        match id {
            Allocate | AllocateLength => {
                if id == Allocate && a[0] == 0 {
                    return Ok(0);
                }
                let text = if id == Allocate {
                    units(mem, a[0], u32::MAX)?
                } else if a[1] > 1 << 20 {
                    return Err(8);
                } else if a[0] == 0 {
                    vec![0; a[1] as usize]
                } else {
                    units(mem, a[0], a[1])?
                };
                strings.allocate(mem, heap, process_heap, &text)
            }
            Length | ByteLength => {
                if a[0] == 0 {
                    return Ok(0);
                }
                let length = mem
                    .read_u32(a[0].checked_sub(4).ok_or(87u32)?)
                    .map_err(|_| 87u32)?;
                Ok(if id == Length { length / 2 } else { length })
            }
            Free => {
                if a[0] == 0 {
                    return Ok(0);
                }
                if !strings.release(heap, process_heap, a[0]).ok_or(87u32)? {
                    return Err(87);
                }
                Ok(0)
            }
            Reallocate | ReallocateLength => {
                let old = mem.read_u32(a[0]).map_err(|_| 87u32)?;
                mem.ptr(a[0], 4, true).map_err(|_| 87u32)?;
                if old != 0 && !strings.owns(old) {
                    return Err(87);
                }
                if id == Reallocate && a[1] == 0 {
                    mem.write_u32(a[0], 0).map_err(|_| 87u32)?;
                    let _ = strings.release(heap, process_heap, old);
                    return Ok(1);
                }
                let length = id == ReallocateLength;
                let text = if a[1] == 0 || (length && old != 0 && a[1] == old) {
                    if length {
                        if a[2] > 1 << 20 {
                            return Err(8);
                        }
                        let mut text = vec![0; a[2] as usize];
                        if old != 0 {
                            let old_length = mem.read_u32(old - 4).map_err(|_| 87u32)? / 2;
                            let prefix = units(mem, old, old_length.min(a[2]))?;
                            text[..prefix.len()].copy_from_slice(&prefix);
                        }
                        text
                    } else {
                        Vec::new()
                    }
                } else {
                    units(mem, a[1], if length { a[2] } else { u32::MAX })?
                };
                let pointer = strings.allocate(mem, heap, process_heap, &text)?;
                mem.write_u32(a[0], pointer).map_err(|_| 87u32)?;
                let _ = strings.release(heap, process_heap, old);
                Ok(1)
            }
            AllocateBytes => unreachable!("PE32 byte allocation remains ABI-only"),
        }
    })();
    match result {
        Ok(value) => (
            value,
            "common process/string/COM allocation contract".into(),
        ),
        Err(error) => {
            *last_error = error;
            (0, format!("common API error {error:#x}"))
        }
    }
}
