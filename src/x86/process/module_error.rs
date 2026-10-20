//! DLL execution may fail in the domain or transfer guest control in Process.
use super::GuestCallOutcome;

pub(crate) enum Failure {
    Expected(u32, String),
    Fatal(String),
    Control(GuestCallOutcome),
}

pub(crate) type LoadResult<T> = Result<T, Failure>;

impl From<crate::modules::Failure> for Failure {
    fn from(failure: crate::modules::Failure) -> Self {
        match failure {
            crate::modules::Failure::Expected(code, message) => Self::Expected(code, message),
            crate::modules::Failure::Fatal(message) => Self::Fatal(message),
        }
    }
}
