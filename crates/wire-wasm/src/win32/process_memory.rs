use super::{call::Call, catalog::ProcessMemoryApi as P, context, ServiceError};
use crate::{
    machine::State,
    memory::{Memory, READ, WRITE},
    process::{current, Loaded},
};

pub(super) fn dispatch(
    id: P,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "process memory requires a loaded image",
    ))?;
    let a = &call.arguments;
    let width = memory.mode() as usize / 8;
    let required = if id == P::Read {
        current::VM_READ
    } else {
        current::VM_OPERATION | current::VM_WRITE
    };
    let result = image.process.check_current(a[0], required).and_then(|()| {
        let length = usize::try_from(a[3]).map_err(|_| 299u32)?;
        if length != 0 && !memory.probe(a[1], length, if id == P::Read { READ } else { WRITE }) {
            return Err(299);
        }
        Ok(length)
    });
    if a[4] != 0 {
        memory.check(a[4], width, WRITE)?;
    }
    match result {
        Err(code) => {
            let error = context::error_address(state, memory.mode());
            memory.check(error, 4, WRITE)?;
            if a[4] != 0 {
                memory.write_int(a[4], width, 0)?;
            }
            memory.write_int(error, 4, u64::from(code))?;
            Ok(0)
        }
        Ok(length) => {
            if length != 0 {
                let (source, destination) = if id == P::Read {
                    (a[1], a[2])
                } else {
                    (a[2], a[1])
                };
                memory.copy(source, destination, length)?;
            }
            if a[4] != 0 {
                memory.write_int(a[4], width, a[3])?;
            }
            Ok(1)
        }
    }
}
