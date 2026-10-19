//! The existing optional pointer obfuscation policy is identity on Win64.
use crate::win32::catalog::PointerHelperApi;

pub(crate) fn call(id: PointerHelperApi, args: &[u64]) -> u64 {
    match id {
        PointerHelperApi::Encode | PointerHelperApi::Decode => args[0],
        _ => unreachable!("PE32 pointer probes/numeric helpers have no Win64 binding"),
    }
}
