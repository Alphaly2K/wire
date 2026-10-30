use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitApi {
    Single,
    SingleEx,
    Multiple,
    MultipleEx,
    SignalAndWait,
}

impl WaitApi {
    pub(crate) fn multiple(self) -> bool {
        matches!(self, Self::Multiple | Self::MultipleEx)
    }
    pub(crate) fn alertable(self) -> bool {
        matches!(
            self,
            Self::SingleEx | Self::MultipleEx | Self::SignalAndWait
        )
    }
}

const fn declaration(
    id: WaitApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(
        ApiId::Wait(id),
        name,
        parameters,
        ReturnConvention::Uint32,
        "existing process-controlled waits, deadlines and retry frames; PE32 preserves count clamp, empty-array results, ignored SingleEx alertable and acquire-before-return-fault; Win64 preserves checked full-width arrays/handles, strict count/access validation, atomic wait-all, APC callbacks, block/unblock and saved return frame",
        [
            ArchitecturePlan {
                dlls: &["kernel32.dll"],
                execution: if matches!(id, WaitApi::MultipleEx) {
                    ExecutionPolicy::LegacyUnimplemented
                } else {
                    ExecutionPolicy::Typed
                },
            },
            ArchitecturePlan {
                dlls: &["kernel32.dll", "kernelbase.dll"],
                execution: ExecutionPolicy::Typed,
            },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};

pub static WAIT_CONTRACTS: [Contract; 5] = [
    Contract::new(ApiId::Wait(WaitApi::SignalAndWait),"SignalObjectAndWait",&[P,P,D,D],ReturnConvention::Uint32,"Signal an owned event/semaphore/mutex exactly once, then use the same cooperative wait/APC/deadline continuation; validate target and output frame before signaling",[ArchitecturePlan {dlls:&["kernel32.dll","kernelbase.dll"],execution:ExecutionPolicy::LegacyUnimplemented};2]),
    declaration(WaitApi::Single, "WaitForSingleObject", &[P, D]),
    declaration(WaitApi::SingleEx, "WaitForSingleObjectEx", &[P, D, D]),
    declaration(WaitApi::Multiple, "WaitForMultipleObjects", &[D, P, D, D]),
    declaration(
        WaitApi::MultipleEx,
        "WaitForMultipleObjectsEx",
        &[D, P, D, D, D],
    ),
];
