//! Checked GUID services; Windows GUIDs store the first three fields little endian.
//! API semantics reviewed against Hangover's Wine combase.c at
//! 3ea3a14941b6a122b892c84141ce9662c008081e (LGPL-2.1-or-later).
//! No Wine allocator, RPC backend or parser implementation is imported.
use super::{Memory, Runtime64, WRITE};

pub(super) fn arity(name: &str) -> Option<usize> {
    Some(match name {
        "CoCreateGuid" => 1,
        "StringFromCLSID" | "StringFromIID" => 2,
        _ => return None,
    })
}
pub(super) fn export(dll: &str, name: &str) -> bool {
    (dll.eq_ignore_ascii_case("ole32.dll") || dll.eq_ignore_ascii_case("combase.dll"))
        && arity(name).is_some()
}
fn text(guid: [u8; 16]) -> Vec<u8> {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}\0",
        u32::from_le_bytes(guid[..4].try_into().unwrap()),
        u16::from_le_bytes(guid[4..6].try_into().unwrap()),
        u16::from_le_bytes(guid[6..8].try_into().unwrap()),
        guid[8],
        guid[9],
        guid[10],
        guid[11],
        guid[12],
        guid[13],
        guid[14],
        guid[15]
    )
    .encode_utf16()
    .flat_map(u16::to_le_bytes)
    .collect()
}
fn parse(units: &[u16]) -> Option<[u8; 16]> {
    if units.len() != 38
        || units[0] != b'{' as u16
        || units[37] != b'}' as u16
        || [9, 14, 19, 24].iter().any(|&n| units[n] != b'-' as u16)
    {
        return None;
    }
    let mut bytes = [0; 16];
    for (n, index) in [1, 3, 5, 7, 10, 12, 15, 17, 20, 22, 25, 27, 29, 31, 33, 35]
        .into_iter()
        .enumerate()
    {
        let hex = |u| char::from_u32(u as u32)?.to_digit(16).map(|v| v as u8);
        bytes[n] = hex(units[index])? * 16 + hex(units[index + 1])?;
    }
    bytes[..4].reverse();
    bytes[4..6].reverse();
    bytes[6..8].reverse();
    Some(bytes)
}
pub(super) fn bound_guid_call(
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
            let data = text(guid);
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
            let data = Runtime64::string_bytes(mem, args[0], u32::MAX as u64, 2)?;
            let units: Vec<_> = data[..data.len() - 2]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            if let Some(guid) = parse(&units) {
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

impl Runtime64 {
    pub(super) fn guid_call(
        &mut self,
        mem: &mut Memory,
        name: &str,
        args: &[u64],
    ) -> Result<u64, String> {
        const INVALIDARG: u64 = 0x80070057;
        match name {
            "CoCreateGuid" => {
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
            "StringFromCLSID" | "StringFromIID" => {
                let mut guid = [0; 16];
                mem.read(args[0], &mut guid)?;
                let data = text(guid);
                mem.check(args[1], 8, WRITE)?;
                let pointer = self.heap_alloc(mem, data.len() as u64)?;
                if pointer == 0 {
                    mem.write_int(args[1], 8, 0)?;
                    return Ok(0x8007000e);
                }
                mem.write(pointer, &data)?;
                self.task_blocks.insert(pointer);
                mem.write_int(args[1], 8, pointer)?;
                Ok(0)
            }

            _ => unreachable!(),
        }
    }
}
