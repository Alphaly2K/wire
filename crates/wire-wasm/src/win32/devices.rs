use super::{
    call::Call,
    catalog::DeviceApi,
    context,
    device_state::Backend,
    modules::{self, Failure},
    ServiceError,
};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};

struct Access<'a>(&'a mut Memory);
impl Backend for Access<'_> {
    type Error = ServiceError;
    fn check_write(&self, address: u64, size: usize) -> Result<(), Self::Error> {
        Ok(self.0.check(address, size, WRITE)?)
    }
    fn read(&self, address: u64, bytes: &mut [u8]) -> Result<(), Self::Error> {
        Ok(self.0.read(address, bytes)?)
    }
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Self::Error> {
        Ok(self.0.write(address, bytes)?)
    }
    fn check_name(&self, address: u64, wide: bool) -> Result<(), Self::Error> {
        modules::string(self.0, address, wide, 32768)
            .map(|_| ())
            .map_err(|error| match error {
                Failure::Fault(f) => ServiceError::Fault(f),
                Failure::Windows(_) => ServiceError::Unavailable("invalid device enumerator name"),
            })
    }
    fn invalid(&self, message: &'static str) -> Self::Error {
        ServiceError::Unavailable(message)
    }
}
pub(super) fn dlls(id: DeviceApi) -> &'static [&'static str] {
    use DeviceApi::*;
    match id {
        RegisterA | RegisterW | Unregister => &["user32.dll"],
        ClassA | ClassW | DestroySet | EnumDevice | EnumInterfaces | DetailA | DetailW => {
            &["setupapi.dll"]
        }
        _ => &["hid.dll"],
    }
}
pub(super) fn dispatch(
    id: DeviceApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable("devices require a loaded image"))?;
    let exists = image.user.windows.contains_key(&call.arguments[0]);
    let error = context::error_address(state, memory.mode());
    image
        .user
        .devices
        .call(&mut Access(memory), id, &call.arguments, error, exists)
}
