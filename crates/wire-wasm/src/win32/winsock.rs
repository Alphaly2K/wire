use super::{
    call::Call,
    catalog::WinsockApi,
    context,
    modules::{self, Failure},
    objects::Objects,
    winsock_core::Backend,
    ServiceError,
};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};
struct Access<'a> {
    memory: &'a mut Memory,
    events: &'a mut Objects,
}
impl Backend for Access<'_> {
    type Error = ServiceError;
    fn width(&self) -> usize {
        self.memory.mode() as usize / 8
    }
    fn check_write(&self, p: u64, n: usize) -> Result<(), ServiceError> {
        Ok(self.memory.check(p, n, WRITE)?)
    }
    fn read(&self, p: u64, bytes: &mut [u8]) -> Result<(), ServiceError> {
        Ok(self.memory.read(p, bytes)?)
    }
    fn write(&mut self, p: u64, bytes: &[u8]) -> Result<(), ServiceError> {
        Ok(self.memory.write(p, bytes)?)
    }
    fn string(&self, p: u64, wide: bool) -> Result<Vec<u8>, ServiceError> {
        let mut bytes = modules::string(self.memory, p, wide, 32768).map_err(|e| match e {
            Failure::Fault(f) => ServiceError::Fault(f),
            Failure::Windows(_) => ServiceError::Unavailable("unterminated Winsock address string"),
        })?;
        bytes.resize(bytes.len() + if wide { 2 } else { 1 }, 0);
        Ok(bytes)
    }
    fn create_event(&mut self) -> Result<(u64, bool), u32> {
        self.events.create(true, false, None, 0x1f0003)
    }
    fn close_event(&mut self, handle: u64) -> bool {
        self.events.close(handle)
    }
    fn change_event(&mut self, handle: u64, signaled: bool) -> Result<u64, u32> {
        self.events.change(handle, signaled)
    }
}
pub(super) fn dispatch(
    id: WinsockApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable("Winsock requires a loaded image"))?;
    if super::winsock_names::handles(id) {
        return super::winsock_names::dispatch(id, &call.arguments, state, memory, image);
    }
    let error = context::error_address(state, memory.mode());
    image.winsock.call(
        &mut Access {
            memory,
            events: &mut image.objects,
        },
        id,
        &call.arguments,
        error,
    )
}
