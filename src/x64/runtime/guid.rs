//! Checked GUID services; Windows GUIDs store the first three fields little endian.
//! API semantics reviewed against Hangover's Wine combase.c at
//! 3ea3a14941b6a122b892c84141ce9662c008081e (LGPL-2.1-or-later).
//! No Wine allocator, RPC backend or parser implementation is imported.
use super::{Memory, Runtime64, WRITE};
use crate::win32::catalog::GuidApi;

impl Runtime64 {
    pub(super) fn guid_call(
        &mut self,
        mem: &mut Memory,
        id: GuidApi,
        args: &[u64],
    ) -> Result<u64, String> {
        const INVALIDARG: u64 = 0x80070057;
        match id {
            GuidApi::Create => {
                if args[0] == 0 {
                    return Ok(INVALIDARG);
                }
                mem.check(args[0], 16, WRITE)?;
                let mut guid = [0; 16];
                if unsafe { libc::getentropy(guid.as_mut_ptr().cast(), guid.len()) } != 0 {
                    return Ok(0x800706b9); // HRESULT_FROM_WIN32(RPC_S_OUT_OF_RESOURCES)
                }
                // Random UUID v4; version is in Data3's high byte in Windows layout.
                guid[7] = (guid[7] & 15) | 0x40;
                guid[8] = (guid[8] & 63) | 0x80;
                mem.write(args[0], &guid)?;
                Ok(0)
            }
            GuidApi::AllocateClsidText | GuidApi::AllocateIidText => {
                let mut guid = [0; 16];
                mem.read(args[0], &mut guid)?;
                let data = crate::win32::com::guid::utf16_bytes(guid);
                mem.check(args[1], 8, WRITE)?;
                let pointer = self.heap_alloc(mem, data.len() as u64)?;
                if pointer == 0 {
                    mem.write_int(args[1], 8, 0)?;
                    return Ok(0x8007000e);
                }
                mem.write(pointer, &data)?;
                self.task_memory.track(pointer);
                mem.write_int(args[1], 8, pointer)?;
                Ok(0)
            }

            _ => crate::win32::x64::guid::call(mem, id, args),
        }
    }
}
