//! Win64 GUID faults propagate before the saved return frame is committed.
use crate::{
    win32::com::guid,
    x64::memory::{Memory, WRITE},
};
pub(crate) fn call(
    mem: &mut Memory,
    id: crate::win32::catalog::GuidApi,
    args: &[u64],
) -> Result<u64, String> {
    use crate::win32::catalog::GuidApi;
    const INVALIDARG: u64 = 0x80070057;
    match id {
        GuidApi::StringFromGuid2 => {
            if args[0] == 0 || (args[2] as u32 as i32) < 39 {
                return Ok(0);
            }
            let mut guid = [0; 16];
            mem.read(args[0], &mut guid)?;
            let data = guid::utf16_bytes(guid);
            mem.check(args[1], data.len(), WRITE)?;
            mem.write(args[1], &data)?;
            Ok(39)
        }
        GuidApi::ClsidFromString | GuidApi::IidFromString => {
            let clsid = id == GuidApi::ClsidFromString;
            if clsid && args[1] == 0 {
                return Ok(INVALIDARG);
            }
            mem.check(args[1], 16, WRITE)?;
            if args[0] == 0 {
                mem.write(args[1], &[0; 16])?;
                return Ok(0);
            }
            // Read raw UTF-16: malformed GUID text is an HRESULT failure,
            // whereas inaccessible or unterminated guest storage is a fault.
            let data = super::text::string_bytes(mem, args[0], u32::MAX as u64, 2)?;
            let units: Vec<_> = data[..data.len() - 2]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            if let Some(guid) = guid::parse(&units) {
                mem.write(args[1], &guid)?;
                return Ok(0);
            }
            if !clsid {
                return Ok(if units.len() != 38 {
                    INVALIDARG
                } else {
                    0x800401f4
                });
            }
            // No activation-context COM registrations or HKCR ProgIDs exist
            // in this isolated profile. Do not invent an installed class.
            if units.first() != Some(&(b'{' as u16)) {
                mem.write(args[1], &[0; 16])?;
            }
            Ok(0x800401f3) // CO_E_CLASSSTRING
        }
    }
}
