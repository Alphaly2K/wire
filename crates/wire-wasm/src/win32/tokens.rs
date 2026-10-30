//! Query handles describe the virtual process identity, independently of host credentials.
use super::{call::Call, catalog::TokenApi as T, context, modules::Failure, ServiceError};
use crate::{
    machine::{Mode, State},
    memory::{Memory, WRITE},
    process::Loaded,
};
#[path = "../../../../src/win32/token_state.rs"]
mod state;
pub(crate) type Tokens = state::Tokens;
pub(crate) const START_X86: u64 = 0xa0000000;

pub(crate) fn new(mode: Mode) -> Tokens {
    let start = if mode == Mode::X86 {
        START_X86
    } else {
        0x7ffd05000000
    };
    Tokens::new(start, start + 4096)
}
fn query(
    id: T,
    a: &[u64; 16],
    cpu: &State,
    memory: &mut Memory,
    image: &mut Loaded,
) -> Result<u64, Failure> {
    match id {
        T::OpenProcess => {
            image.process.query(a[0]).map_err(Failure::Windows)?;
            let access = a[1] as u32;
            let handle = image.tokens.prepare(access).map_err(Failure::Windows)?;
            memory.write_int(a[2], memory.mode() as usize / 8, handle)?;
            image.tokens.commit(handle, access, false);
            Ok(1)
        }
        T::OpenThread => {
            let thread = image
                .threads
                .target(a[0], 0x840)
                .map_err(Failure::Windows)?;
            let level = image
                .threads
                .get(thread)
                .and_then(|t| t.impersonation)
                .ok_or(Failure::Windows(1008))?;
            let access = a[1] as u32;
            let handle = image.tokens.prepare(access).map_err(Failure::Windows)?;
            memory.write_int(a[3], memory.mode() as usize / 8, handle)?;
            image
                .tokens
                .commit_level(handle, access, false, Some(level));
            Ok(1)
        }
        T::Information => {
            image.tokens.query(a[0]).map_err(Failure::Windows)?;
            let bytes = state::information(
                a[1] as u32,
                a[2],
                memory.mode() as usize / 8,
                image.tokens.level(a[0]).map_err(Failure::Windows)?,
            )
            .map_err(Failure::Windows)?;
            memory.check(a[4], 4, WRITE)?;
            if (a[3] as u32 as usize) < bytes.len() {
                let error = context::error_address(cpu, memory.mode());
                memory.check(error, 4, WRITE)?;
                memory.write_int(a[4], 4, bytes.len() as u64)?;
                memory.write_int(error, 4, 122)?;
                return Ok(0);
            }
            memory.check(a[2], bytes.len(), WRITE)?;
            memory.write(a[2], &bytes)?;
            memory.write_int(a[4], 4, bytes.len() as u64)?;
            Ok(1)
        }
    }
}
pub(super) fn dispatch(
    id: T,
    call: &Call,
    cpu: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "token services require a loaded image",
    ))?;
    match query(id, &call.arguments, cpu, memory, image) {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(fault.into()),
        Err(Failure::Windows(error)) => {
            memory.write_int(context::error_address(cpu, memory.mode()), 4, error.into())?;
            Ok(0)
        }
    }
}
