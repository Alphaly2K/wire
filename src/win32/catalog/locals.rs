use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalApi {
    TlsAlloc,
    TlsGet,
    TlsSet,
    TlsFree,
    FlsAlloc,
    FlsGet,
    FlsSet,
    FlsFree,
}

const fn declaration(
    id: LocalApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Local(id),
        name,
        parameters,
        returns,
        "PE32 preserves monotonic indices, permissive thread-keyed maps and ignored FLS callbacks; Win64 preserves bounded slots, full-width TEB values, live-thread allocation preflight, TLS hot routes and rejection of nonzero FLS values requiring callbacks",
        [
            ArchitecturePlan {
                dlls: &["kernel32.dll"],
                execution: ExecutionPolicy::Typed,
            },
            ArchitecturePlan {
                dlls: &["kernel32.dll", "kernelbase.dll"],
                execution: ExecutionPolicy::Typed,
            },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as V, Uint32 as I};

pub static LOCAL_CONTRACTS: [Contract; 8] = [
    declaration(LocalApi::TlsAlloc, "TlsAlloc", &[], I),
    declaration(LocalApi::TlsGet, "TlsGetValue", &[D], V),
    declaration(LocalApi::TlsSet, "TlsSetValue", &[D, P], B),
    declaration(LocalApi::TlsFree, "TlsFree", &[D], B),
    declaration(LocalApi::FlsAlloc, "FlsAlloc", &[P], I),
    declaration(LocalApi::FlsGet, "FlsGetValue", &[D], V),
    declaration(LocalApi::FlsSet, "FlsSetValue", &[D, P], B),
    declaration(LocalApi::FlsFree, "FlsFree", &[D], B),
];
