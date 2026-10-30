use super::{
    call::Call,
    catalog::SspiApi,
    data_pages,
    modules::{self, Failure},
    sspi_state::{self, Backend},
    ServiceError, Services,
};
use crate::{
    memory::{Memory, WRITE},
    process::Loaded,
};
struct Access<'a> {
    memory: &'a mut Memory,
    pages: &'a mut data_pages::Pages,
    services: &'a mut Services,
}
impl Backend for Access<'_> {
    type Error = ServiceError;
    fn width(&self) -> usize {
        self.memory.mode() as usize / 8
    }
    fn install(&mut self, wide: bool) -> Result<u64, Self::Error> {
        let mut staged = self.services.clone();
        let mut gateways = Vec::new();
        for (slot, name) in sspi_state::exports(wide) {
            let gateway = staged
                .bind("secur32.dll", &name)
                .map_err(|_| ServiceError::Unavailable("SSPI gateway capacity exhausted"))?;
            gateways.push((slot, gateway));
        }
        let bytes = sspi_state::encode(self.width(), &gateways);
        let address = self.pages.allocate(self.memory, bytes.len() as u64);
        if address == 0 {
            return Ok(0);
        }
        self.memory
            .write(address, &bytes)
            .expect("fresh SSPI table backing");
        self.pages.readonly(self.memory, address)?;
        *self.services = staged;
        Ok(address)
    }
    fn check_write(&self, address: u64, length: usize) -> Result<(), Self::Error> {
        Ok(self.memory.check(address, length, WRITE)?)
    }
    fn write_int(&mut self, address: u64, width: usize, value: u64) -> Result<(), Self::Error> {
        Ok(self.memory.write_int(address, width, value)?)
    }
    fn check_name(&self, address: u64, wide: bool) -> Result<(), Self::Error> {
        let invalid = || ServiceError::Unavailable("invalid security package name");
        let bytes =
            modules::string(self.memory, address, wide, 32768).map_err(|error| match error {
                Failure::Fault(f) => ServiceError::Fault(f),
                Failure::Windows(_) => invalid(),
            })?;
        if wide {
            let units: Vec<_> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_le_bytes)
                .collect();
            String::from_utf16(&units).map_err(|_| invalid())?;
        } else {
            crate::codepage::decode(&bytes, 932).map_err(|_| invalid())?;
        }
        Ok(())
    }
}
pub(super) fn dispatch(
    id: SspiApi,
    call: &Call,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
    services: &mut Services,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable("SSPI requires a loaded image"))?;
    image.security.invoke(
        &mut Access {
            memory,
            pages: &mut image.data_pages,
            services,
        },
        id,
        &call.arguments,
    )
}
