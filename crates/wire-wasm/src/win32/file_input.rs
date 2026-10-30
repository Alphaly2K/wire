use super::{call::Call, ServiceError};
use crate::{
    filesystem::Files,
    process::{threads::PendingKind, Loaded},
};

pub(super) fn saved(image: &Loaded, pc: u64) -> Option<&Call> {
    image
        .pending()
        .filter(|pending| pending.pc == pc && matches!(pending.kind, PendingKind::FileInput))
        .map(|pending| &pending.call)
}
pub(super) fn suspend(
    files: &Files,
    image: Option<&mut Loaded>,
    call: &Call,
    pc: u64,
) -> Option<ServiceError> {
    let request = files.pending_input()?;
    if let Some(image) = image {
        image.set_pending(call.clone(), PendingKind::FileInput, pc);
    }
    Some(ServiceError::FileInput(request))
}
