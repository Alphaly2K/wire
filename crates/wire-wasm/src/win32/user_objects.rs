use super::{
    call::Call,
    catalog::UserObjectApi,
    context,
    modules::{self, Failure},
    user::{
        object_services,
        objects::{Properties, RegisteredNames},
        User,
    },
    ServiceError,
};
use crate::{machine::State, memory::Memory, process::Loaded};

struct Backend<'a> {
    user: &'a mut User,
    memory: &'a mut Memory,
    error: u64,
}
impl object_services::Backend for Backend<'_> {
    type Error = Failure;
    fn name(&self, pointer: u64, wide: bool) -> Result<String, Failure> {
        modules::name(self.memory, pointer, wide)
    }
    fn names(&mut self) -> &mut RegisteredNames {
        &mut self.user.registered
    }
    fn valid_window(&self, window: u64) -> bool {
        self.user.windows.contains_key(&window)
    }
    fn properties(&mut self, window: u64) -> &mut Properties {
        self.user.properties.entry(window).or_default()
    }
    fn error(&mut self, code: u32) -> Result<(), Failure> {
        self.memory.write_int(self.error, 4, code.into())?;
        Ok(())
    }
}
pub(super) fn dispatch(
    id: UserObjectApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&mut Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable(
        "USER objects require a loaded image",
    ))?;
    let mut backend = Backend {
        user: &mut image.user,
        error: context::error_address(state, memory.mode()),
        memory,
    };
    match object_services::call(&mut backend, id, &call.arguments) {
        Ok((value, _)) => Ok(value),
        Err(Failure::Fault(fault)) => Err(fault.into()),
        Err(Failure::Windows(code)) => {
            backend.memory.write_int(backend.error, 4, code.into())?;
            Ok(0)
        }
    }
}
