//! Virtual directory queries use guest defaults and guest-owned temp variables.
use super::{
    call::Call, catalog::SystemPathApi as S, context, environment::Environment, ServiceError,
};
use crate::{filesystem::Files, machine::State, memory::Memory};

pub(super) fn dispatch(
    id: S,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    environment: &Environment,
    files: &Files,
) -> Result<u64, ServiceError> {
    let path = if id.temporary() {
        let text = ["TMP", "TEMP", "USERPROFILE"]
            .into_iter()
            .find_map(|n| environment.get(n).filter(|v| !v.is_empty()))
            .unwrap_or(environment.profile.windows_directory());
        match files.resolve(text) {
            Ok(path) => path.trim_end_matches('\\').to_owned() + "\\",
            Err(code) => {
                memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
                return Ok(0);
            }
        }
    } else if id.system() {
        environment.profile.system_directory().into()
    } else {
        environment.profile.windows_directory().into()
    };
    let (pointer, capacity) = if id.temporary() {
        (call.arguments[1], call.arguments[0])
    } else {
        (call.arguments[0], call.arguments[1])
    };
    let bytes: Vec<u8> = if id.wide() {
        path.encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    } else {
        crate::codepage::encode(&(path + "\0"), environment.profile.codepage())
            .expect("virtual path codepage")
            .0
    };
    let required = bytes.len() / if id.wide() { 2 } else { 1 };
    if capacity < required as u64 {
        return Ok(required as u64);
    }
    memory.write(pointer, &bytes)?;
    Ok(required as u64 - 1)
}
