use super::{
    call::Call,
    catalog::ImageApi,
    context,
    modules::{self, Failure},
    resources::Reader,
    user::{image_services, images, resources},
    ServiceError,
};
use crate::{machine::State, memory::Memory, pe::linker::Linked, process::Loaded};

struct Backend<'a> {
    memory: &'a mut Memory,
    modules: &'a Linked,
    error: u64,
}
impl image_services::Backend for Backend<'_> {
    type Error = Failure;
    fn key(&self, pointer: u64, wide: bool) -> Result<images::Key, Failure> {
        if pointer <= 65535 {
            Ok(images::Key::Id(pointer as u32))
        } else {
            modules::name(self.memory, pointer, wide).map(images::Key::Name)
        }
    }
    fn valid_module(&self, base: u64) -> bool {
        self.modules.modules.contains_key(&base)
    }
    fn resource(
        &self,
        base: u64,
        kind: u32,
        key: &images::Key,
    ) -> Result<Option<Vec<u8>>, Failure> {
        let image = &self
            .modules
            .modules
            .get(&base)
            .ok_or(Failure::Windows(6))?
            .image;
        let dir = image.dir(2);
        resources::resource(
            &Reader(self.memory),
            base,
            image.size,
            dir.rva,
            dir.size,
            kind,
            key,
        )
    }
    fn invalid(&self, _message: String) -> Failure {
        Failure::Windows(13)
    }
    fn error(&mut self, code: u32) -> Result<(), Failure> {
        self.memory.write_int(self.error, 4, code.into())?;
        Ok(())
    }
}
pub(super) fn dispatch(
    id: ImageApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable("images require a loaded image"))?;
    let mut backend = Backend {
        error: context::error_address(state, memory.mode()),
        memory,
        modules: &image.modules,
    };
    match image_services::call(&mut image.user.images, &mut backend, id, &call.arguments) {
        Ok(value) => Ok(value),
        Err(Failure::Fault(fault)) => Err(fault.into()),
        Err(Failure::Windows(code)) => {
            image_services::Backend::error(&mut backend, code).map_err(|e| match e {
                Failure::Fault(fault) => ServiceError::Fault(fault),
                Failure::Windows(_) => unreachable!(),
            })?;
            Ok(0)
        }
    }
}
