use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterlockedApi {
    Increment,
    Decrement,
    Exchange,
    CompareExchange,
    InitializeSListHead,
    FlushSList,
}

const fn declaration(
    id: InterlockedApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
    win64: bool,
) -> Contract {
    Contract::new(
        ApiId::Interlocked(id), name, parameters, returns,
        "cooperative guest execution; PE32 wrapping LONG operations and aligned 8-byte SLIST headers; failed compare needs no write; Win64 only aligned 16-byte initialization; preserve checked access, LastError and architecture-specific return-frame order",
        [
            ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::Typed },
            ArchitecturePlan {
                dlls: if win64 { &["kernel32.dll", "kernelbase.dll"] } else { &[] },
                execution: if win64 { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent },
            },
        ],
    )
}

use Parameter::{Int32 as I, Pointer as P};
use ReturnConvention::{Pointer as R, Uint32 as U};

pub static INTERLOCKED_CONTRACTS: [Contract; 6] = [
    declaration(
        InterlockedApi::Increment,
        "InterlockedIncrement",
        &[P],
        U,
        false,
    ),
    declaration(
        InterlockedApi::Decrement,
        "InterlockedDecrement",
        &[P],
        U,
        false,
    ),
    declaration(
        InterlockedApi::Exchange,
        "InterlockedExchange",
        &[P, I],
        U,
        false,
    ),
    declaration(
        InterlockedApi::CompareExchange,
        "InterlockedCompareExchange",
        &[P, I, I],
        U,
        false,
    ),
    declaration(
        InterlockedApi::InitializeSListHead,
        "InitializeSListHead",
        &[P],
        ReturnConvention::Void,
        true,
    ),
    declaration(
        InterlockedApi::FlushSList,
        "InterlockedFlushSList",
        &[P],
        R,
        false,
    ),
];
