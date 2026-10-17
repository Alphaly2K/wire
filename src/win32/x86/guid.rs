//! PE32 GUID errors and bounded UTF-16 reads preserve the legacy facade contract.
use crate::{
    api::Args,
    heap::GuestHeap,
    mem::GuestMem,
    win32::{catalog::GuidApi, com::guid},
};

pub(crate) fn call(
    id: GuidApi,
    a: &Args,
    mem: &mut GuestMem,
    heap: &mut GuestHeap,
    process_heap: u32,
) -> Result<u32, u32> {
    match id {
        GuidApi::Create => {
            mem.ptr(a[0], 16, true).map_err(|_| 0x80004003u32)?;
            let mut guid = [0; 16];
            if unsafe { libc::getentropy(guid.as_mut_ptr().cast(), guid.len()) } != 0 {
                return Err(0x80004005);
            }
            guid[7] = (guid[7] & 15) | 0x40;
            guid[8] = (guid[8] & 63) | 0x80;
            mem.write_bytes(a[0], &guid).map_err(|_| 0x80004003u32)?;
            Ok(0)
        }
        GuidApi::AllocateClsidText | GuidApi::AllocateIidText => {
            let mut value = [0; 16];
            mem.read_bytes(a[0], &mut value)
                .map_err(|_| 0x80004003u32)?;
            let bytes = guid::utf16_bytes(value);
            mem.write_u32(a[1], 0).map_err(|_| 0x80004003u32)?;
            let pointer = heap
                .alloc(mem, process_heap, 78, false)
                .map_err(|_| 0x8007000eu32)?;
            if pointer == 0 {
                return Err(0x8007000e);
            }
            if mem.write_bytes(pointer, &bytes).is_err() || mem.write_u32(a[1], pointer).is_err() {
                heap.free(process_heap, pointer);
                return Err(0x80004003);
            }
            Ok(0)
        }
        GuidApi::StringFromGuid2 => {
            if (a[2] as i32) < 39 {
                return Ok(0);
            }
            let mut guid = [0; 16];
            mem.read_bytes(a[0], &mut guid).map_err(|_| 0x80004003u32)?;
            let bytes = guid::utf16_bytes(guid);
            Ok(if mem.write_bytes(a[1], &bytes).is_ok() {
                39
            } else {
                0
            })
        }
        GuidApi::ClsidFromString | GuidApi::IidFromString => {
            mem.ptr(a[1], 16, true).map_err(|_| 0x80004003u32)?;
            let mut guid = [0; 16];
            if a[0] != 0 {
                let text = mem.read_wstr(a[0], 39).map_err(|_| 0x800401f3u32)?;
                guid =
                    guid::parse(&text.encode_utf16().collect::<Vec<_>>()).ok_or(0x800401f3u32)?;
            }
            mem.write_bytes(a[1], &guid).map_err(|_| 0x80004003u32)?;
            Ok(0)
        }
    }
}
