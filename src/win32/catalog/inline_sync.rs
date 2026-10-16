use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineSyncApi {
    InitializeSrw,
    AcquireExclusive,
    AcquireShared,
    ReleaseExclusive,
    ReleaseShared,
    TryExclusive,
    TryShared,
    InitializeCondition,
    WakeCondition,
    WakeAllCondition,
    BeginOnce,
    CompleteOnce,
}

const fn declaration(
    id: InlineSyncApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::InlineSync(id), name, parameters, returns,
        "PE32 ABI-only error 120; Win64 checked 8-byte inline state, SRW process ownership and scheduler retry, nonzero condition wait-list rejection, synchronous InitOnce with ordered output checks/writes, saved return frame and failed Try without unblock",
        [
            ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::LegacyUnimplemented },
            ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Uint32 as U, Void as V};
pub static INLINE_SYNC_CONTRACTS: [Contract; 12] = [
    declaration(InlineSyncApi::InitializeSrw, "InitializeSRWLock", &[P], V),
    declaration(
        InlineSyncApi::AcquireExclusive,
        "AcquireSRWLockExclusive",
        &[P],
        V,
    ),
    declaration(
        InlineSyncApi::AcquireShared,
        "AcquireSRWLockShared",
        &[P],
        V,
    ),
    declaration(
        InlineSyncApi::ReleaseExclusive,
        "ReleaseSRWLockExclusive",
        &[P],
        V,
    ),
    declaration(
        InlineSyncApi::ReleaseShared,
        "ReleaseSRWLockShared",
        &[P],
        V,
    ),
    declaration(
        InlineSyncApi::TryExclusive,
        "TryAcquireSRWLockExclusive",
        &[P],
        U,
    ),
    declaration(InlineSyncApi::TryShared, "TryAcquireSRWLockShared", &[P], U),
    declaration(
        InlineSyncApi::InitializeCondition,
        "InitializeConditionVariable",
        &[P],
        V,
    ),
    declaration(
        InlineSyncApi::WakeCondition,
        "WakeConditionVariable",
        &[P],
        V,
    ),
    declaration(
        InlineSyncApi::WakeAllCondition,
        "WakeAllConditionVariable",
        &[P],
        V,
    ),
    declaration(
        InlineSyncApi::BeginOnce,
        "InitOnceBeginInitialize",
        &[P, D, P, P],
        B,
    ),
    declaration(
        InlineSyncApi::CompleteOnce,
        "InitOnceComplete",
        &[P, D, P],
        B,
    ),
];
