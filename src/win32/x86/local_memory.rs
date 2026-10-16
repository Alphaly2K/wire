//! PE32 Local/Global calls retain their shared process-heap resource.
use crate::{api::Args, heap::GuestHeap, mem::GuestMem, win32::catalog::LocalMemoryApi};

pub(crate) fn call(
    id: LocalMemoryApi,
    args: &Args,
    mem: &mut GuestMem,
    heap: &mut GuestHeap,
    process_heap: u32,
) -> Result<u32, String> {
    Ok(match id {
        LocalMemoryApi::Alloc => {
            if args[0] & 2 != 0 {
                return Err("movable Local/GlobalAlloc is unsupported".into());
            }
            heap.alloc(mem, process_heap, args[1], args[0] & 0x40 != 0)?
        }
        LocalMemoryApi::Free => {
            if heap.free(process_heap, args[0]) {
                0
            } else {
                args[0]
            }
        }
        LocalMemoryApi::Size => heap.size(process_heap, args[0]).unwrap_or(0),
        LocalMemoryApi::Lock => args[0],
        LocalMemoryApi::Unlock => 0,
        LocalMemoryApi::ReAlloc | LocalMemoryApi::Flags => {
            unreachable!("PE32 Local/Global ReAlloc and Flags keep ABI-only error 120")
        }
    })
}
