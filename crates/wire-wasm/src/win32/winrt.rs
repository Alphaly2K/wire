use super::{
    call::Call,
    catalog::WinrtApi,
    data_pages,
    hstrings_core::{self, Backend},
    ServiceError,
};
use crate::{
    memory::{Memory, READ, WRITE},
    process::Loaded,
};
struct Access<'a> {
    memory: &'a mut Memory,
    pages: &'a mut data_pages::Pages,
}
impl Backend for Access<'_> {
    type Error = ServiceError;
    fn width(&self) -> usize {
        self.memory.mode() as usize / 8
    }
    fn check(&self, address: u64, length: usize, write: bool) -> Result<(), Self::Error> {
        Ok(self
            .memory
            .check(address, length, if write { WRITE } else { READ })?)
    }
    fn read(&self, address: u64, bytes: &mut [u8]) -> Result<(), Self::Error> {
        Ok(self.memory.read(address, bytes)?)
    }
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Self::Error> {
        Ok(self.memory.write(address, bytes)?)
    }
    fn allocate(&mut self, size: u64) -> Result<u64, Self::Error> {
        Ok(self.pages.allocate(self.memory, size))
    }
    fn readonly(&mut self, address: u64) -> Result<(), Self::Error> {
        self.pages.readonly(self.memory, address)
    }
    fn free(&mut self, address: u64) -> Result<(), Self::Error> {
        self.pages.free(self.memory, address)
    }
    fn invalid(&self, message: &'static str) -> Self::Error {
        ServiceError::Unavailable(message)
    }
    fn activation(&mut self, _: &[u16]) {}
}
pub(super) fn dlls(id: WinrtApi) -> &'static [&'static str] {
    if matches!(id, WinrtApi::ActivationFactory | WinrtApi::ActivateInstance) {
        &["combase.dll", "api-ms-win-core-winrt-l1-1-0.dll"]
    } else {
        &["combase.dll", "api-ms-win-core-winrt-string-l1-1-0.dll"]
    }
}
pub(super) fn dispatch(
    id: WinrtApi,
    call: &Call,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "WinRT strings require a loaded image",
    ))?;
    image.hstrings.invoke(
        &mut Access {
            memory,
            pages: &mut image.data_pages,
        },
        id,
        &call.arguments,
    )
}
pub(crate) type Strings = hstrings_core::Strings;
