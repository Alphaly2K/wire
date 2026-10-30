use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterlockedApi {
    Increment,
    Decrement,
    Exchange,
    CompareExchange,
    InitializeSListHead,
    FlushSList,
    PopSList,
    PushSList,
}

const fn declaration(
    id: InterlockedApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
    architectures: [bool; 2],
) -> Contract {
    Contract::new(
        ApiId::Interlocked(id), name, parameters, returns,
        "cooperative guest execution; PE32 wrapping LONG operations and aligned 8-byte SLIST headers; failed compare needs no write; Win64 aligned 16-byte SLIST headers and indivisible entry push/pop; preserve checked access, LastError and architecture-specific return-frame order",
        [
            ArchitecturePlan {
                dlls: if architectures[0] { &["kernel32.dll"] } else { &[] },
                execution: if architectures[0] { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent },
            },
            ArchitecturePlan {
                dlls: if architectures[1] { &["kernel32.dll", "kernelbase.dll"] } else { &[] },
                execution: if architectures[1] { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent },
            },
        ],
    )
}

use Parameter::{Int32 as I, Pointer as P};
use ReturnConvention::{Pointer as R, Uint32 as U};

pub static INTERLOCKED_CONTRACTS: [Contract; 8] = [
    declaration(
        InterlockedApi::Increment,
        "InterlockedIncrement",
        &[P],
        U,
        [true, false],
    ),
    declaration(
        InterlockedApi::Decrement,
        "InterlockedDecrement",
        &[P],
        U,
        [true, false],
    ),
    declaration(
        InterlockedApi::Exchange,
        "InterlockedExchange",
        &[P, I],
        U,
        [true, false],
    ),
    declaration(
        InterlockedApi::CompareExchange,
        "InterlockedCompareExchange",
        &[P, I, I],
        U,
        [true, false],
    ),
    declaration(
        InterlockedApi::InitializeSListHead,
        "InitializeSListHead",
        &[P],
        ReturnConvention::Void,
        [true, true],
    ),
    declaration(
        InterlockedApi::FlushSList,
        "InterlockedFlushSList",
        &[P],
        R,
        [true, false],
    ),
    declaration(
        InterlockedApi::PopSList,
        "InterlockedPopEntrySList",
        &[P],
        R,
        [false, true],
    ),
    declaration(
        InterlockedApi::PushSList,
        "InterlockedPushEntrySList",
        &[P, P],
        R,
        [false, true],
    ),
];
