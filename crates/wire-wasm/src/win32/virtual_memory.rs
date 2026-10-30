//! Windows VMM calls share Memory's allocation, protection and source authority.
use super::{call::Call, catalog::VirtualMemoryApi as V, context, ServiceError};
use crate::{
    machine::{Mode, State},
    memory::{Memory, WRITE},
    process::{current, Loaded},
};

fn process_access(
    memory: &Memory,
    image: Option<&Loaded>,
    handle: u64,
    required: u32,
) -> Result<(), u32> {
    if handle == memory.mode().address(u64::MAX) {
        Ok(())
    } else {
        image.ok_or(6u32)?.process.check_current(handle, required)
    }
}

pub(super) fn dispatch(
    id: V,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&Loaded>,
) -> Result<u64, ServiceError> {
    if id.extended() {
        let required = if id.base() == V::Query {
            current::QUERY
        } else {
            current::VM_OPERATION
        };
        if let Err(error) = process_access(memory, image, call.arguments[0], required) {
            return finish(Err(error), state, memory);
        }
    }
    let result = {
        let a = &call.arguments[usize::from(id.extended())..];
        match id.base() {
            V::Alloc => memory.virtual_alloc(a[0], a[1], a[2] as u32, a[3] as u32),
            V::Free => memory.virtual_free(a[0], a[1], a[2] as u32).map(|()| 1),
            V::Protect => {
                memory.check(a[3], 4, WRITE)?;
                match memory.validate_protect(a[0], a[1], a[2] as u32) {
                    Ok(old) => {
                        memory.write_int(a[3], 4, u64::from(old))?;
                        memory.virtual_protect(a[0], a[1], a[2] as u32).map(|_| 1)
                    }
                    Err(error) => Err(error),
                }
            }
            V::Query => {
                let x86 = memory.mode() == Mode::X86;
                let length = if x86 { 28 } else { 48 };
                if a[2] < length || x86 && a[1] == 0 {
                    Err(if x86 { 87 } else { 24 })
                } else {
                    match memory.virtual_query(a[0]) {
                        Ok(info) => {
                            if x86 {
                                let bytes = match info.encode32() {
                                    Ok(bytes) => bytes,
                                    Err(error) => return finish(Err(error), state, memory),
                                };
                                memory.write(a[1], &bytes)?;
                            } else {
                                memory.write(a[1], &info.encode64())?;
                            }
                            Ok(length)
                        }
                        Err(error) => Err(error),
                    }
                }
            }
            _ => unreachable!(),
        }
    };
    finish(result, state, memory)
}

pub(super) fn flush(
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&Loaded>,
) -> Result<u64, ServiceError> {
    finish(
        process_access(memory, image, call.arguments[0], current::VM_WRITE).map(|()| 1),
        state,
        memory,
    )
}

fn finish(
    result: Result<u64, u32>,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            memory.write_int(
                context::error_address(state, memory.mode()),
                4,
                u64::from(error),
            )?;
            Ok(0)
        }
    }
}
