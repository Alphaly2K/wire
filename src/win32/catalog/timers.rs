use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerApi {
    CreateA,
    CreateW,
    CreateExA,
    CreateExW,
    OpenA,
    OpenW,
    Set,
    SetEx,
    Cancel,
}

impl TimerApi {
    pub(crate) fn creating(self) -> bool {
        matches!(
            self,
            Self::CreateA | Self::CreateW | Self::CreateExA | Self::CreateExW
        )
    }
    pub(crate) fn opening(self) -> bool {
        matches!(self, Self::OpenA | Self::OpenW)
    }
    pub(crate) fn extended(self) -> bool {
        matches!(self, Self::CreateExA | Self::CreateExW)
    }
    pub(crate) fn wide(self) -> bool {
        matches!(self, Self::CreateW | Self::CreateExW | Self::OpenW)
    }
}

const fn declaration(
    id: TimerApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Timer(id), name, parameters, returns,
        "PE32 remains ABI-only error 120; Win64 keeps typed shared named-object namespace, per-alias access, monotonic deadlines and auto/manual signals, due-time read before argument/handle checks, existing APC/reason-context rejection and LastError/output/return-frame order",
        [
            ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::LegacyUnimplemented },
            ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed },
        ],
    )
}

use Parameter::{Int32 as I, Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as H};

pub static TIMER_CONTRACTS: [Contract; 9] = [
    declaration(TimerApi::CreateA, "CreateWaitableTimerA", &[P, D, P], H),
    declaration(TimerApi::CreateW, "CreateWaitableTimerW", &[P, D, P], H),
    declaration(
        TimerApi::CreateExA,
        "CreateWaitableTimerExA",
        &[P, P, D, D],
        H,
    ),
    declaration(
        TimerApi::CreateExW,
        "CreateWaitableTimerExW",
        &[P, P, D, D],
        H,
    ),
    declaration(TimerApi::OpenA, "OpenWaitableTimerA", &[D, D, P], H),
    declaration(TimerApi::OpenW, "OpenWaitableTimerW", &[D, D, P], H),
    declaration(TimerApi::Set, "SetWaitableTimer", &[P, P, I, P, P, D], B),
    declaration(
        TimerApi::SetEx,
        "SetWaitableTimerEx",
        &[P, P, I, P, P, P, D],
        B,
    ),
    declaration(TimerApi::Cancel, "CancelWaitableTimer", &[P], B),
];
