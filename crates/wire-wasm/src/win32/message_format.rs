//! Message preparation precedes checked publication through caller or local storage.
use super::{
    call::Call,
    catalog::MessageFormatApi as M,
    context,
    heaps::{Error, Heaps},
    message_text,
    modules::{name, Failure},
    ServiceError,
};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
};

fn prepare(id: M, call: &Call, memory: &Memory) -> Result<Vec<u8>, Failure> {
    let a = &call.arguments;
    let flags = a[0] as u32;
    let source = flags & 0x1c00;
    if flags & !0x3fff != 0 || source == 0 || source & 0x400 != 0 && source != 0x400 {
        return Err(Failure::Windows(87));
    }
    if flags & 0xff != 0 || flags & 0x800 != 0 || !matches!(a[3], 0 | 0x400 | 0x409) {
        return Err(Failure::Windows(50));
    }
    let text = if source == 0x400 {
        name(memory, a[1], id.wide())?
    } else {
        let message = message_text::system(a[2] as u32).ok_or(Failure::Windows(317))?;
        format!("{message}\r\n")
    };
    if flags & 0x200 == 0 && (a[6] != 0 || text.contains('%')) {
        return Err(Failure::Windows(50));
    }
    if id.wide() {
        Ok(text
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect())
    } else {
        crate::codepage::encode(&(text + "\0"), 932)
            .map(|(bytes, _)| bytes)
            .map_err(Failure::Windows)
    }
}

fn call(id: M, call: &Call, memory: &mut Memory, heaps: Option<&mut Heaps>) -> Result<u64, Error> {
    let bytes = prepare(id, call, memory).map_err(|failure| match failure {
        Failure::Windows(code) => Error::Windows(code),
        Failure::Fault(fault) => Error::Fault(fault),
    })?;
    let a = &call.arguments;
    let width = if id.wide() { 2 } else { 1 };
    let count = (bytes.len() / width - 1) as u64;
    if a[0] & 0x100 != 0 {
        let pointer_width = memory.mode() as usize / 8;
        memory.check(a[4], pointer_width, WRITE)?;
        let size = (count + 1).max(a[5]) * width as u64;
        let pointer = heaps
            .ok_or(Error::Backing)?
            .allocate_local(memory, size, &bytes)?;
        memory
            .write_int(a[4], pointer_width, pointer)
            .expect("preflighted message pointer");
    } else {
        if a[5] <= count {
            return Err(Error::Windows(122));
        }
        memory.write(a[4], &bytes)?;
    }
    Ok(count)
}

pub(super) fn dispatch(
    id: M,
    call_frame: &Call,
    state: &State,
    memory: &mut Memory,
    heaps: Option<&mut Heaps>,
) -> Result<u64, ServiceError> {
    match call(id, call_frame, memory, heaps) {
        Ok(value) => Ok(value),
        Err(Error::Fault(fault)) => Err(ServiceError::Fault(fault)),
        Err(Error::Backing) => Err(ServiceError::Unavailable(
            "message allocation requires available process-heap backing",
        )),
        Err(Error::Windows(code)) => {
            memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
            Ok(0)
        }
    }
}
