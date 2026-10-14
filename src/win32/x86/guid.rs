//! PE32 GUID errors and bounded UTF-16 reads preserve the legacy facade contract.
use crate::{
    api::Args,
    mem::GuestMem,
    win32::{catalog::GuidApi, com::guid},
};

pub(crate) fn call(id: GuidApi, a: &Args, mem: &mut GuestMem) -> Result<u32, u32> {
    match id {
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
