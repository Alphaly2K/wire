//! GUID text is encoded in the core; randomness crosses only the stopped service boundary.
use super::{
    call::Call,
    catalog::GuidApi as G,
    com, guid_values,
    modules::{self, Failure},
    ServiceError,
};
use crate::{
    memory::{Memory, WRITE},
    process::Loaded,
};

pub(super) fn dispatch(
    id: G,
    call: &Call,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
    random: Option<&[u8]>,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    Ok(match id {
        G::Create => {
            if a[0] == 0 {
                return Ok(0x80070057);
            }
            memory.check(a[0], 16, WRITE)?;
            let Some(mut bytes) = random.and_then(|bytes| <[u8; 16]>::try_from(bytes).ok()) else {
                return Ok(0x800706b9);
            };
            bytes[7] = bytes[7] & 15 | 0x40;
            bytes[8] = bytes[8] & 63 | 0x80;
            memory.write(a[0], &bytes)?;
            0
        }
        G::StringFromGuid2 => {
            if a[0] == 0 || (a[2] as u32 as i32) < 39 {
                return Ok(0);
            }
            let mut value = [0; 16];
            memory.read(a[0], &mut value)?;
            memory.write(a[1], &guid_values::utf16_bytes(value))?;
            39
        }
        G::ClsidFromString | G::IidFromString => {
            let clsid = id == G::ClsidFromString;
            if clsid && a[1] == 0 {
                return Ok(0x80070057);
            }
            if a[0] == 0 {
                memory.write(a[1], &[0; 16])?;
                return Ok(0);
            }
            let bytes = match modules::string(memory, a[0], true, 32768) {
                Ok(bytes) => bytes,
                Err(Failure::Fault(fault)) => return Err(fault.into()),
                Err(Failure::Windows(_)) => {
                    return Err(ServiceError::Unavailable(
                        "GUID input exceeds string capacity",
                    ))
                }
            };
            let units: Vec<_> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|v| u16::from_le_bytes(*v))
                .collect();
            if let Some(value) = guid_values::parse(&units) {
                memory.write(a[1], &value)?;
                0
            } else if clsid {
                if units.first() != Some(&123) {
                    memory.write(a[1], &[0; 16])?;
                }
                0x800401f3
            } else if units.len() != 38 {
                0x80070057
            } else {
                0x800401f4
            }
        }
        G::AllocateClsidText | G::AllocateIidText => {
            let image = image.ok_or(ServiceError::Unavailable(
                "allocated GUID text requires a loaded image",
            ))?;
            let mut value = [0; 16];
            memory.read(a[0], &mut value)?;
            memory.check(a[1], memory.mode() as usize / 8, WRITE)?;
            let bytes = guid_values::utf16_bytes(value);
            let pointer = com::allocation(image.heaps.allocate_task(
                memory,
                bytes.len() as u64,
                Some(&bytes),
            ))?;
            memory.write_int(a[1], memory.mode() as usize / 8, pointer)?;
            if pointer == 0 {
                0x8007000e
            } else {
                0
            }
        }
    })
}
