use super::{call::Call, catalog::EtwApi, ServiceError};
use crate::{
    memory::{Fault, Memory, READ, WRITE},
    process::Loaded,
};

impl super::tracing_state::Memory for Memory {
    type Error = Fault;
    fn check(&self, address: u64, length: usize, write: bool) -> Result<(), Self::Error> {
        Memory::check(self, address, length, if write { WRITE } else { READ })
    }
    fn read_guid(&self, address: u64) -> Result<[u8; 16], Self::Error> {
        let mut guid = [0; 16];
        self.read(address, &mut guid)?;
        Ok(guid)
    }
    fn write_handle(&mut self, address: u64, handle: u64) -> Result<(), Self::Error> {
        self.write_int(address, 8, handle)
    }
}

pub(super) fn dispatch(
    id: EtwApi,
    call: &Call,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    Ok(image
        .ok_or(ServiceError::Unavailable("ETW requires a loaded image"))?
        .etw
        .call(memory, id, &call.arguments)?)
}
