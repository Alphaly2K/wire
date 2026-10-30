use super::{call::Call, catalog::WinHttpApi, context, winhttp_core, ServiceError};
use crate::{
    machine::State,
    memory::{Fault, Memory, WRITE},
};
impl winhttp_core::Memory for Memory {
    type Error = Fault;
    fn check_write(&self, address: u64, length: usize) -> Result<(), Self::Error> {
        self.check(address, length, WRITE)
    }
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Self::Error> {
        Memory::write(self, address, bytes)
    }
    fn write_error(&mut self, address: u64, code: u64) -> Result<(), Self::Error> {
        self.write_int(address, 4, code)
    }
}
pub(super) fn dispatch(
    id: WinHttpApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    Ok(winhttp_core::call(
        memory,
        id,
        &call.arguments,
        context::error_address(state, memory.mode()),
        memory.mode() as usize / 8,
    )?)
}
