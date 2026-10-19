//! File version resources retain their PE32-only provider and output policy.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionResourceApi {
    InfoA,
    InfoW,
    SizeA,
    SizeW,
    QueryA,
    QueryW,
}
impl VersionResourceApi {
    pub(crate) const fn wide(self) -> bool {
        matches!(self, Self::InfoW | Self::SizeW | Self::QueryW)
    }
    pub(crate) const fn fetch(self) -> bool {
        !matches!(self, Self::QueryA | Self::QueryW)
    }
    pub(crate) const fn size(self) -> bool {
        matches!(self, Self::SizeA | Self::SizeW)
    }
}

const fn declaration(
    id: VersionResourceApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::VersionResource(id), name, parameters, returns,
        "PE32 version.dll only; checked confined file resource extraction, existing blob expansion, ignored legacy handle, output preflight and length clearing before blob faults, CP932 converted scratch inside caller buffer, sequential pointer/length writes and unsaved return-slot aliases; Win64 absent",
        [ArchitecturePlan { dlls: &["version.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }],
    )
}
use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Uint32 as U};
pub static VERSION_RESOURCE_CONTRACTS: [Contract; 6] = [
    declaration(
        VersionResourceApi::InfoA,
        "GetFileVersionInfoA",
        &[P, D, D, P],
        B,
    ),
    declaration(
        VersionResourceApi::InfoW,
        "GetFileVersionInfoW",
        &[P, D, D, P],
        B,
    ),
    declaration(
        VersionResourceApi::SizeA,
        "GetFileVersionInfoSizeA",
        &[P, P],
        U,
    ),
    declaration(
        VersionResourceApi::SizeW,
        "GetFileVersionInfoSizeW",
        &[P, P],
        U,
    ),
    declaration(
        VersionResourceApi::QueryA,
        "VerQueryValueA",
        &[P, P, P, P],
        B,
    ),
    declaration(
        VersionResourceApi::QueryW,
        "VerQueryValueW",
        &[P, P, P, P],
        B,
    ),
];
