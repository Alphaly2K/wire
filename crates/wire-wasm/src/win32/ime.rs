use super::{
    call::Call,
    catalog::ImeApi,
    user::{self, input_profile as profile},
    ServiceError,
};
use crate::{machine::State, memory::Memory, process::Loaded};
pub(super) fn dispatch(
    id: ImeApi,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&Loaded>,
) -> Result<u64, ServiceError> {
    let image = image.ok_or(ServiceError::Unavailable("IME requires a loaded image"))?;
    let result = profile::ime(
        id,
        &call.arguments,
        image.user.windows.contains_key(&call.arguments[0]),
    );
    if let Some(code) = result.error {
        user::error(memory, state, code)?;
    }
    Ok(result.value)
}
