use super::{
    call::Call,
    catalog::ThreadControlApi,
    heaps::Error,
    modules::{self, Failure},
    ServiceError,
};
use crate::{
    memory::{Memory, WRITE},
    process::Loaded,
};

fn hresult(code: u32) -> u64 {
    u64::from(
        0x10000000u32
            | match code {
                0 => 0,
                5 => 0xc0000022,
                6 => 0xc0000008,
                8 | 14 => 0xc0000017,
                _ => 0xc000000d,
            },
    )
}

pub(super) fn dispatch(
    id: ThreadControlApi,
    call: &Call,
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, ServiceError> {
    let [handle, pointer, ..] = call.arguments;
    let set = id == ThreadControlApi::SetDescription;
    let width = memory.mode() as usize / 8;
    if !set {
        memory.check(pointer, width, WRITE)?;
    }
    let thread = match image
        .threads
        .target(handle, if set { 0x420 } else { 0x840 })
    {
        Ok(id) => id,
        Err(code) => {
            if !set {
                memory.write_int(pointer, width, 0)?;
            }
            return Ok(hresult(code));
        }
    };
    if set {
        let bytes = if pointer == 0 {
            Vec::new()
        } else {
            match modules::string(memory, pointer, true, 32768) {
                Ok(bytes) => bytes,
                Err(Failure::Fault(fault)) => return Err(ServiceError::Fault(fault)),
                Err(Failure::Windows(code)) => return Ok(hresult(code)),
            }
        };
        image.threads.get_mut(thread).description = bytes;
    } else {
        let mut bytes = image.threads.get(thread).unwrap().description.clone();
        bytes.extend_from_slice(&[0, 0]);
        let result = image
            .heaps
            .allocate_local(memory, bytes.len() as u64, &bytes);
        let value = match result {
            Ok(value) => value,
            Err(Error::Fault(fault)) => return Err(ServiceError::Fault(fault)),
            Err(Error::Backing) => {
                return Err(ServiceError::Unavailable(
                    "thread description requires available process-heap backing",
                ))
            }
            Err(Error::Windows(code)) => {
                memory.write_int(pointer, width, 0)?;
                return Ok(hresult(code));
            }
        };
        memory
            .write_int(pointer, width, value)
            .expect("preflighted thread-description pointer");
    }
    Ok(hresult(0))
}
