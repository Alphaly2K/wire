//! Virtual identity has no connection to host accounts or machine names.
use super::{call::Call, catalog::IdentityApi as I, context, ServiceError};
use crate::{
    environment::GuestEnvironment,
    machine::{Mode, State},
    memory::{Memory, WRITE},
};

pub(super) fn dispatch(
    id: I,
    call: &Call,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    let profile = if memory.mode() == Mode::X86 {
        GuestEnvironment::pe32()
    } else {
        GuestEnvironment::amd64()
    };
    let a = &call.arguments;
    let text = if id.user() {
        profile.user_name()
    } else if id.extended() {
        match a[0] as u32 {
            0 | 4 => profile.computer_name(),
            1 | 3 | 5 | 7 => profile.dns_hostname(),
            2 | 6 => profile.dns_domain(),
            _ => {
                memory.write_int(context::error_address(state, memory.mode()), 4, 87)?;
                return Ok(0);
            }
        }
    } else {
        profile.computer_name()
    };
    let offset = usize::from(id.extended());
    let output = a[offset];
    let size = a[offset + 1];
    let capacity = memory.read_int(size, 4)?;
    let bytes: Vec<_> = if id.wide() {
        text.encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    } else {
        text.bytes().chain(Some(0)).collect()
    };
    let required = (bytes.len() / if id.wide() { 2 } else { 1 }) as u64;
    memory.check(size, 4, WRITE)?;
    if capacity < required {
        let error = context::error_address(state, memory.mode());
        memory.check(error, 4, WRITE)?;
        memory.write_int(size, 4, required)?;
        memory.write_int(
            error,
            4,
            if id.user() {
                122
            } else if id.extended() {
                234
            } else {
                111
            },
        )?;
        return Ok(0);
    }
    memory.check(output, bytes.len(), WRITE)?;
    memory.write(output, &bytes)?;
    memory.write_int(size, 4, if id.user() { required } else { required - 1 })?;
    Ok(1)
}
