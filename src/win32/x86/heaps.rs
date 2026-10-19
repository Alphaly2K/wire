//! PE32 heap adapter borrows the allocator and error slot, not ApiState.
use crate::{api::Args, heap::GuestHeap, mem::GuestMem, win32::catalog::HeapApi};

pub(crate) fn call(
    id: HeapApi,
    args: &Args,
    mem: &mut GuestMem,
    heap: &mut GuestHeap,
    process_heap: u32,
    last_error: &mut u32,
) -> Result<(u32, String), String> {
    let mut note = String::new();
    let ret = match id {
        HeapApi::Process => process_heap,
        HeapApi::Alloc => {
            let value = heap.alloc(mem, args[0], args[2], args[1] & 8 != 0)?;
            if value == 0 {
                *last_error = 8;
            }
            value
        }
        HeapApi::ReAlloc => {
            let value = heap.realloc(mem, args[0], args[2], args[3], args[1])?;
            if value == 0 {
                *last_error = 8;
            }
            value
        }
        HeapApi::Free => {
            let value = heap.free(args[0], args[2]) as u32;
            if value == 0 {
                *last_error = 87;
            }
            value
        }
        HeapApi::Destroy => {
            let value = (args[0] != process_heap && heap.destroy(args[0])) as u32;
            if value == 0 {
                *last_error = 87;
            }
            value
        }
        HeapApi::Create => heap.create(args[2]),
        HeapApi::Size => heap.size(args[0], args[2]).unwrap_or(u32::MAX),
        HeapApi::Validate => heap.valid(args[0], args[2]) as u32,
        HeapApi::Enumerate => {
            note = "fake: single heap".into();
            if args[0] >= 1 {
                mem.write_u32(args[1], process_heap).ok();
            }
            1
        }
        HeapApi::SetInformation | HeapApi::QueryInformation | HeapApi::Compact | HeapApi::Walk => {
            unreachable!("ABI-only heap extensions keep the gateway error 120 route")
        }
    };
    Ok((ret, note))
}

/// CRT allocation keeps its existing flags, overflow and free return policy.
pub(crate) fn crt_call(
    name: &str,
    args: &Args,
    mem: &mut GuestMem,
    heap: &mut GuestHeap,
    process_heap: u32,
) -> Result<Option<u32>, String> {
    let value = match name {
        "malloc" => heap.alloc(mem, process_heap, args[0], false)?,
        "calloc" => match args[0].checked_mul(args[1]) {
            Some(n) => heap.alloc(mem, process_heap, n, true)?,
            None => 0,
        },
        "realloc" => {
            if args[0] == 0 {
                heap.alloc(mem, process_heap, args[1], false)?
            } else if args[1] == 0 {
                heap.free(process_heap, args[0]);
                0
            } else {
                heap.realloc(mem, process_heap, args[0], args[1], 0)?
            }
        }
        "free" | "_free_dbg" => {
            heap.free(process_heap, args[0]);
            0
        }
        "_strdup" | "strdup" => {
            let mut bytes = Vec::new();
            for i in 0..1 << 20 {
                let b = mem
                    .read_u8(args[0].checked_add(i).ok_or("strdup address overflow")?)
                    .map_err(|e| e.to_string())?;
                bytes.push(b);
                if b == 0 {
                    break;
                }
            }
            if bytes.last() != Some(&0) {
                return Err("unterminated strdup source".into());
            }
            let p = heap.alloc(mem, process_heap, bytes.len() as u32, false)?;
            if p != 0 {
                mem.write_bytes(p, &bytes).map_err(|e| e.to_string())?;
            }
            p
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}
