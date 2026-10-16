use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultimediaApi {
    Caps,
    BeginPeriod,
    EndPeriod,
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
pub static MULTIMEDIA_CONTRACTS: [Contract; 3] = [
    declaration(MultimediaApi::Caps, "timeGetDevCaps", &[P, D]),
    declaration(MultimediaApi::BeginPeriod, "timeBeginPeriod", &[D]),
    declaration(MultimediaApi::EndPeriod, "timeEndPeriod", &[D]),
];
