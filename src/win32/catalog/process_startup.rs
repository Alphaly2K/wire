//! Architecture-specific process startup storage, layouts and feature replies.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupApi {
    CommandA,
    CommandW,
    InfoA,
    InfoW,
    Feature,
}

const fn declaration(
    id: StartupApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Startup(id), name, parameters, returns,
        "existing process-owned command storage; STARTUPINFO writes exactly 68 bytes on PE32 and 104 on Win64 with cb set and remaining bytes zero; return carrier stays zero; PE32 output precedes reading return frame, Win64 uses saved return frame; preserve error/fault/API trace order; feature DWORD retains PE32 {6,10} and Win64 {12} replies; provider scope remains architecture-specific",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{Pointer as H, Uint32 as U, Void};
pub static STARTUP_CONTRACTS: [Contract; 5] = [
    declaration(StartupApi::CommandA, "GetCommandLineA", &[], H),
    declaration(StartupApi::CommandW, "GetCommandLineW", &[], H),
    declaration(StartupApi::InfoA, "GetStartupInfoA", &[P], Void),
    declaration(StartupApi::InfoW, "GetStartupInfoW", &[P], Void),
    declaration(StartupApi::Feature, "IsProcessorFeaturePresent", &[D], U),
];
