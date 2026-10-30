use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultimediaApi {
    Caps,
    BeginPeriod,
    EndPeriod,
    SetEvent,
    KillEvent,
}

const fn declaration(
    id: MultimediaApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(
        ApiId::Multimedia(id), name, parameters, ReturnConvention::Uint32,
        "virtual timer capabilities 1..UINT_MAX; PE32 period calls accept nonzero without bookkeeping and convert output faults to 97, Win64 balances requests with 256 distinct periods and checked reference counts, retains saved frame and checked copy-out; Win64 static kernel imports remain allowed but dynamic lookup is winmm-only",
        [
            ArchitecturePlan { dlls: &["winmm.dll"], execution: ExecutionPolicy::Typed },
            ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll", "winmm.dll"], execution: ExecutionPolicy::Typed },
        ],
    ).with_lookup_dlls([&["winmm.dll"], &["winmm.dll"]])
}

use Parameter::{Pointer as P, Uint32 as D};
pub static MULTIMEDIA_CONTRACTS: [Contract; 5] = [
    Contract::new(ApiId::Multimedia(MultimediaApi::SetEvent),"timeSetEvent",&[D,D,P,P,D],ReturnConvention::Uint32,"No multimedia callback/event timer owner or backend; zero timer ID, no callback/event and unchanged LastError",[ArchitecturePlan {dlls:&["winmm.dll"],execution:ExecutionPolicy::LegacyUnimplemented};2]),
    Contract::new(ApiId::Multimedia(MultimediaApi::KillEvent),"timeKillEvent",&[D],ReturnConvention::Uint32,"No allocated multimedia timer ID: TIMERR_NOCANDO(97), no effect or LastError change",[ArchitecturePlan {dlls:&["winmm.dll"],execution:ExecutionPolicy::LegacyUnimplemented};2]),
    declaration(MultimediaApi::Caps, "timeGetDevCaps", &[P, D]),
    declaration(MultimediaApi::BeginPeriod, "timeBeginPeriod", &[D]),
    declaration(MultimediaApi::EndPeriod, "timeEndPeriod", &[D]),
];
