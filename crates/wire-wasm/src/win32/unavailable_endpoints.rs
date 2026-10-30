use super::{audio, call::Call, catalog::ApiId, context, ServiceError};
use crate::{machine::State, memory::Memory};

pub(super) fn dispatch(
    id: ApiId,
    call: &Call,
    state: &State,
    memory: &mut Memory,
) -> Result<u64, ServiceError> {
    match id {
        ApiId::Audio(id) => {
            let width = memory.mode() as usize / 8;
            audio::call(id, &call.arguments, width, |output, bytes| {
                memory.write(output, bytes)
            })
            .map(|result| result.value.into())
            .map_err(Into::into)
        }
        ApiId::FontResource(_) => {
            memory.write_int(context::error_address(state, memory.mode()), 4, 120)?;
            Ok(0)
        }
        _ => unreachable!(),
    }
}
