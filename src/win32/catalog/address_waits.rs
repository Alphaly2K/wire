use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressWaitApi {
    Wait,
    WakeSingle,
    WakeAll,
}

const fn declaration(
    id: AddressWaitApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::AddressWait(id), name, parameters, returns,
        "PE32 absent, preserving unknown ABI and zero argument cleanup; Win64 cooperative arrival-ordered waits with full SIZE_T and DWORD timeout, size-before-retry validation, no comparison reread on retry, no wake credit, saved return frame, per-thread LastError, existing deadline and notification order",
        [
            ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent },
            ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll", "api-ms-win-core-synch-l1-2-0.dll"], execution: ExecutionPolicy::Typed },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
pub static ADDRESS_WAIT_CONTRACTS: [Contract; 3] = [
    declaration(
        AddressWaitApi::Wait,
        "WaitOnAddress",
        &[P, P, P, D],
        ReturnConvention::BoolLastError,
    ),
    declaration(
        AddressWaitApi::WakeSingle,
        "WakeByAddressSingle",
        &[P],
        ReturnConvention::Void,
    ),
    declaration(
        AddressWaitApi::WakeAll,
        "WakeByAddressAll",
        &[P],
        ReturnConvention::Void,
    ),
];
