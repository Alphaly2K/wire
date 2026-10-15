use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelayApi {
    Sleep,
    SleepEx,
    SwitchThread,
}

const fn declaration(
    id: DelayApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Delay(id),
        name,
        parameters,
        returns,
        "cooperative scheduling with architecture-specific yield, deadline, wake and return-frame order; PE32 SleepEx ignores alertable; Win64 SleepEx retains APC delivery, saved deadlines and block control, Sleep/SwitchThread retain distinct process outcomes and trace/counter order",
        [
            ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::Typed },
            ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed },
        ],
    )
}

use Parameter::Uint32 as D;
use ReturnConvention::{Uint32, Void};

pub static DELAY_CONTRACTS: [Contract; 3] = [
    declaration(DelayApi::Sleep, "Sleep", &[D], Void),
    declaration(DelayApi::SleepEx, "SleepEx", &[D, D], Uint32),
    declaration(DelayApi::SwitchThread, "SwitchToThread", &[], Uint32),
];
