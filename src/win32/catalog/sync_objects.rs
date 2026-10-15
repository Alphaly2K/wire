use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncObjectApi {
    CreateEventA,
    CreateEventW,
    CreateEventExA,
    CreateEventExW,
    OpenEventA,
    OpenEventW,
    SetEvent,
    ResetEvent,
    PulseEvent,
    CreateSemaphoreA,
    CreateSemaphoreW,
    CreateSemaphoreExA,
    CreateSemaphoreExW,
    OpenSemaphoreA,
    OpenSemaphoreW,
    ReleaseSemaphore,
}

impl SyncObjectApi {
    pub(crate) fn wide(self) -> bool {
        matches!(
            self,
            Self::CreateEventW
                | Self::CreateEventExW
                | Self::OpenEventW
                | Self::CreateSemaphoreW
                | Self::CreateSemaphoreExW
                | Self::OpenSemaphoreW
        )
    }
    pub(crate) fn extended(self) -> bool {
        matches!(
            self,
            Self::CreateEventExA
                | Self::CreateEventExW
                | Self::CreateSemaphoreExA
                | Self::CreateSemaphoreExW
        )
    }
    pub(crate) fn opening(self) -> bool {
        matches!(
            self,
            Self::OpenEventA | Self::OpenEventW | Self::OpenSemaphoreA | Self::OpenSemaphoreW
        )
    }
    const fn pe32_typed(self) -> bool {
        !matches!(
            self,
            Self::CreateEventExA
                | Self::CreateEventExW
                | Self::CreateSemaphoreExA
                | Self::CreateSemaphoreExW
                | Self::OpenSemaphoreA
                | Self::OpenSemaphoreW
        )
    }
}

const fn declaration(
    id: SyncObjectApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let win64 = !matches!(id, SyncObjectApi::PulseEvent);
    Contract::new(
        ApiId::SyncObject(id),
        name,
        parameters,
        returns,
        "existing event/semaphore subsets; PE32 cross-kind named handles, permissive counts, pulse and mutation before output/return faults; Win64 per-alias access, strict counts, shared typed namespace, SECURITY_ATTRIBUTES rejection and output-before-release order; wait scheduling remains in the process adapters",
        [
            ArchitecturePlan {
                dlls: &["kernel32.dll"],
                execution: if id.pe32_typed() {
                    ExecutionPolicy::Typed
                } else {
                    ExecutionPolicy::LegacyUnimplemented
                },
            },
            ArchitecturePlan {
                dlls: if win64 {
                    &["kernel32.dll", "kernelbase.dll"]
                } else {
                    &[]
                },
                execution: if win64 {
                    ExecutionPolicy::Typed
                } else {
                    ExecutionPolicy::Absent
                },
            },
        ],
    )
}

use Parameter::{Int32 as I, Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as H};

pub static SYNC_OBJECT_CONTRACTS: [Contract; 16] = [
    declaration(
        SyncObjectApi::CreateEventA,
        "CreateEventA",
        &[P, D, D, P],
        H,
    ),
    declaration(
        SyncObjectApi::CreateEventW,
        "CreateEventW",
        &[P, D, D, P],
        H,
    ),
    declaration(
        SyncObjectApi::CreateEventExA,
        "CreateEventExA",
        &[P, P, D, D],
        H,
    ),
    declaration(
        SyncObjectApi::CreateEventExW,
        "CreateEventExW",
        &[P, P, D, D],
        H,
    ),
    declaration(SyncObjectApi::OpenEventA, "OpenEventA", &[D, D, P], H),
    declaration(SyncObjectApi::OpenEventW, "OpenEventW", &[D, D, P], H),
    declaration(SyncObjectApi::SetEvent, "SetEvent", &[P], B),
    declaration(SyncObjectApi::ResetEvent, "ResetEvent", &[P], B),
    declaration(SyncObjectApi::PulseEvent, "PulseEvent", &[P], B),
    declaration(
        SyncObjectApi::CreateSemaphoreA,
        "CreateSemaphoreA",
        &[P, I, I, P],
        H,
    ),
    declaration(
        SyncObjectApi::CreateSemaphoreW,
        "CreateSemaphoreW",
        &[P, I, I, P],
        H,
    ),
    declaration(
        SyncObjectApi::CreateSemaphoreExA,
        "CreateSemaphoreExA",
        &[P, I, I, P, D, D],
        H,
    ),
    declaration(
        SyncObjectApi::CreateSemaphoreExW,
        "CreateSemaphoreExW",
        &[P, I, I, P, D, D],
        H,
    ),
    declaration(
        SyncObjectApi::OpenSemaphoreA,
        "OpenSemaphoreA",
        &[D, D, P],
        H,
    ),
    declaration(
        SyncObjectApi::OpenSemaphoreW,
        "OpenSemaphoreW",
        &[D, D, P],
        H,
    ),
    declaration(
        SyncObjectApi::ReleaseSemaphore,
        "ReleaseSemaphore",
        &[P, I, P],
        B,
    ),
];
