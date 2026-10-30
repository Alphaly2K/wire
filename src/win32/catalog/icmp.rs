use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IcmpApi {
    Create,
    Create6,
    Close,
    Echo,
    Echo2,
    Echo2Ex,
    Echo6,
    Parse,
    Parse6,
}

const fn declaration(
    id: IcmpApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(ApiId::Icmp(id), name, parameters, returns,
        "No ICMP transport: constructors return full-width INVALID_HANDLE_VALUE and ERROR_NOT_SUPPORTED; operations have no valid handle; no reply, event, APC, wait or clock sample",
        [ArchitecturePlan { dlls: &["iphlpapi.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2])
}
use Parameter::{Pointer as P, Uint16 as W, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as R, Uint32 as U};
pub static ICMP_CONTRACTS: [Contract; 9] = [
    declaration(IcmpApi::Create, "IcmpCreateFile", &[], R),
    declaration(IcmpApi::Create6, "Icmp6CreateFile", &[], R),
    declaration(IcmpApi::Close, "IcmpCloseHandle", &[P], B),
    declaration(IcmpApi::Echo, "IcmpSendEcho", &[P, D, P, W, P, P, D, D], U),
    declaration(
        IcmpApi::Echo2,
        "IcmpSendEcho2",
        &[P, P, P, P, D, P, W, P, P, D, D],
        U,
    ),
    declaration(
        IcmpApi::Echo2Ex,
        "IcmpSendEcho2Ex",
        &[P, P, P, P, D, D, P, W, P, P, D, D],
        U,
    ),
    declaration(
        IcmpApi::Echo6,
        "Icmp6SendEcho2",
        &[P, P, P, P, P, P, P, W, P, P, D, D],
        U,
    ),
    declaration(IcmpApi::Parse, "IcmpParseReplies", &[P, D], U),
    declaration(IcmpApi::Parse6, "Icmp6ParseReplies", &[P, D], U),
];
