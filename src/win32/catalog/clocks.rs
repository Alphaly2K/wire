use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockApi {
    TickCount,
    TickCount64,
    MultimediaTick,
    PerformanceCounter,
    PerformanceFrequency,
    SystemFileTime,
}

const fn declaration(
    id: ClockApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let multimedia = matches!(id, ClockApi::MultimediaTick);
    Contract::new(
        ApiId::Clock(id), name, parameters, returns,
        "process-relative monotonic clock and 10MHz counter; architecture-specific wall-time conversion and checked output/return-frame order; PE32 TickCount64 commits EDX:EAX; preserve Win64 hot path and multimedia static-import/lookup difference",
        [
            ArchitecturePlan {
                dlls: if multimedia { &["winmm.dll"] } else { &["kernel32.dll"] },
                execution: ExecutionPolicy::Typed,
            },
            ArchitecturePlan {
                dlls: if multimedia { &["kernel32.dll", "kernelbase.dll", "winmm.dll"] }
                      else { &["kernel32.dll", "kernelbase.dll"] },
                execution: ExecutionPolicy::Typed,
            },
        ],
    ).with_lookup_dlls([
        if multimedia { &["winmm.dll"] } else { &["kernel32.dll"] },
        if multimedia { &["winmm.dll"] } else { &["kernel32.dll", "kernelbase.dll"] },
    ])
}

use Parameter::Pointer as P;
use ReturnConvention::{BoolLastError as B, Uint32 as D, Uint64 as Q, Void};

pub static CLOCK_CONTRACTS: [Contract; 6] = [
    declaration(ClockApi::TickCount, "GetTickCount", &[], D),
    declaration(ClockApi::TickCount64, "GetTickCount64", &[], Q),
    declaration(ClockApi::MultimediaTick, "timeGetTime", &[], D),
    declaration(
        ClockApi::PerformanceCounter,
        "QueryPerformanceCounter",
        &[P],
        B,
    ),
    declaration(
        ClockApi::PerformanceFrequency,
        "QueryPerformanceFrequency",
        &[P],
        B,
    ),
    declaration(
        ClockApi::SystemFileTime,
        "GetSystemTimeAsFileTime",
        &[P],
        Void,
    ),
];
