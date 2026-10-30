//! Optional OS backends have no objects to publish or events to deliver.
use super::{
    call::Call,
    catalog::{AccessibilityApi as A, ApiId, CertificateApi as C, ConsoleApi as K, IcmpApi as I},
    user, ServiceError,
};
use crate::{
    machine::State,
    memory::{Memory, WRITE},
    process::Loaded,
};

pub(super) fn dispatch(
    id: ApiId,
    call: &Call,
    state: &State,
    memory: &mut Memory,
    image: Option<&Loaded>,
) -> Result<u64, ServiceError> {
    let a = &call.arguments;
    let width = memory.mode() as usize / 8;
    match id {
        ApiId::Console(K::Free) => Ok(1),
        ApiId::Console(K::Attach | K::ControlHandler) => user::error(memory, state, 50),
        ApiId::Console(_) => user::error(memory, state, 6),
        ApiId::Certificate(C::OpenStore | C::AddEncoded | C::VerifyPolicy) => {
            user::error(memory, state, 50)
        }
        ApiId::Certificate(C::CloseStore) if a[0] == 0 => Ok(1),
        ApiId::Certificate(C::CloseStore) => user::error(memory, state, 6),
        ApiId::Certificate(C::FreeContext | C::FreeChain) if a[0] == 0 => {
            Ok(u64::from(id == ApiId::Certificate(C::FreeContext)))
        }
        ApiId::Certificate(C::FreeContext | C::FreeChain) => Err(ServiceError::Unavailable(
            "certificate release requires an owned certificate context",
        )),
        ApiId::Certificate(C::GetChain) => {
            if a[7] == 0 {
                return user::error(memory, state, 87);
            }
            memory.check(a[7], width, WRITE)?;
            user::error(memory, state, 50)?;
            memory.write_int(a[7], width, 0)?;
            Ok(0)
        }
        ApiId::Icmp(I::Create | I::Create6) => {
            user::error(memory, state, 50)?;
            Ok(memory.mode().address(u64::MAX))
        }
        ApiId::Icmp(I::Parse | I::Parse6) => user::error(memory, state, 50),
        ApiId::Icmp(_) => user::error(memory, state, 6),
        ApiId::Accessibility(A::Clients | A::ReturnProvider) => Ok(0),
        ApiId::Accessibility(A::HostProvider) => {
            if a[1] == 0 {
                return Ok(0x80004003);
            }
            memory.check(a[1], width, WRITE)?;
            memory.write_int(a[1], width, 0)?;
            Ok(
                if image.is_some_and(|image| image.user.windows.contains_key(&a[0])) {
                    0x80004001
                } else {
                    0x80070006
                },
            )
        }
        ApiId::Accessibility(_) => Ok(if a[0] == 0 { 0x80070057 } else { 0x80004001 }),
        _ => Err(ServiceError::Unbound),
    }
}
