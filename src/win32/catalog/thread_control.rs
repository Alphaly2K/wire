use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadControlApi {
    Create,
    Exit,
    FreeAndExit,
    QueueApc,
    Context,
    SetDescription,
    GetDescription,
}

const fn declaration(
    id: ThreadControlApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
    pe32_typed: bool,
) -> Contract {
    Contract::new(
        ApiId::ThreadControl(id), name, parameters, returns,
        "cooperative threads; PE32 creation commits before ID output, exit notifications before TLS/USER release, optional controls ABI-only; Win64 scheduler admission, full-width stack/handle/APC data, saved return frame, explicit exit/context process outcomes, APC callback preflight before queue mutation, HRESULT descriptions owned by LocalFree",
        [
            ArchitecturePlan {
                dlls: &["kernel32.dll"],
                execution: if pe32_typed { ExecutionPolicy::Typed } else { ExecutionPolicy::LegacyUnimplemented },
            },
            ArchitecturePlan {
                dlls: &["kernel32.dll", "kernelbase.dll"],
                execution: ExecutionPolicy::Typed,
            },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Hresult as H, Pointer as R, Uint32 as U, Void as V};

pub static THREAD_CONTROL_CONTRACTS: [Contract; 7] = [
    declaration(
        ThreadControlApi::Create,
        "CreateThread",
        &[P, P, P, P, D, P],
        R,
        true,
    ),
    declaration(ThreadControlApi::Exit, "ExitThread", &[D], V, true),
    declaration(
        ThreadControlApi::FreeAndExit,
        "FreeLibraryAndExitThread",
        &[P, D],
        V,
        false,
    ),
    declaration(
        ThreadControlApi::QueueApc,
        "QueueUserAPC",
        &[P, P, P],
        U,
        false,
    ),
    declaration(
        ThreadControlApi::Context,
        "GetThreadContext",
        &[P, P],
        B,
        false,
    ),
    declaration(
        ThreadControlApi::SetDescription,
        "SetThreadDescription",
        &[P, P],
        H,
        false,
    ),
    declaration(
        ThreadControlApi::GetDescription,
        "GetThreadDescription",
        &[P, P],
        H,
        false,
    ),
];
