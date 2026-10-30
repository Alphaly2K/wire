use super::{
    basic_string_core, call::Call, catalog::BasicStringApi, context, modules::Failure, ServiceError,
};
use crate::{
    machine::State,
    memory::{Fault, FaultKind, Memory, READ},
};

struct Adapter<'a>(&'a mut Memory);
impl basic_string_core::Memory for Adapter<'_> {
    type Error = Failure;
    fn add(&self, pointer: u64, offset: u64) -> Result<u64, Failure> {
        pointer
            .checked_add(offset)
            .filter(|&p| self.0.mode().address(p) == p)
            .ok_or(Failure::Fault(Fault {
                address: pointer,
                length: offset as usize,
                access: READ,
                kind: FaultKind::Address,
            }))
    }
    fn read(&self, pointer: u64, output: &mut [u8]) -> Result<(), Failure> {
        self.0.read(pointer, output).map_err(Failure::Fault)
    }
    fn write(&mut self, pointer: u64, bytes: &[u8]) -> Result<(), Failure> {
        self.0.write(pointer, bytes).map_err(Failure::Fault)
    }
    fn invalid(&self) -> Failure {
        Failure::Windows(87)
    }
}
pub(super) fn dispatch(
    id: BasicStringApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    match basic_string_core::call(id, &call.arguments, &mut Adapter(memory)) {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(fault.into()),
        Err(Failure::Windows(code)) => {
            memory.write_int(context::error_address(state, memory.mode()), 4, code.into())?;
            Ok(0)
        }
    }
}
