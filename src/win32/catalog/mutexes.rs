use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutexApi {
    CreateA,
    CreateW,
    CreateExA,
    CreateExW,
    OpenA,
    OpenW,
    Release,
}

const fn declaration(
    id: MutexApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Mutex(id),
        name,
        parameters,
        returns,
        "PE32 legacy shared named-handle namespace, recursive ownership by scheduler TID, error 288 for nonowners and release/wake before return-frame faults; extended create and open remain ABI-only; Win64 declares ABI for adapters that explicitly support mutex ownership",
        [
            ArchitecturePlan {
                dlls: &["kernel32.dll"],
                execution: if matches!(id, MutexApi::CreateA | MutexApi::CreateW | MutexApi::Release) {
                    ExecutionPolicy::Typed
                } else {
                    ExecutionPolicy::LegacyUnimplemented
                },
            },
            ArchitecturePlan {
                dlls: &["kernel32.dll", "kernelbase.dll"],
                execution: ExecutionPolicy::LegacyUnimplemented,
            },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as H};

pub static MUTEX_CONTRACTS: [Contract; 7] = [
    declaration(MutexApi::CreateA, "CreateMutexA", &[P, D, P], H),
    declaration(MutexApi::CreateW, "CreateMutexW", &[P, D, P], H),
    declaration(MutexApi::CreateExA, "CreateMutexExA", &[P, P, D, D], H),
    declaration(MutexApi::CreateExW, "CreateMutexExW", &[P, P, D, D], H),
    declaration(MutexApi::OpenA, "OpenMutexA", &[D, D, P], H),
    declaration(MutexApi::OpenW, "OpenMutexW", &[D, D, P], H),
    declaration(MutexApi::Release, "ReleaseMutex", &[P], B),
];
