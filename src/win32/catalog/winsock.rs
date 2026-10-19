//! Winsock metadata and Win64-only ordinal normalization; no transport backend.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinsockApi {
    Startup,
    Cleanup,
    GetLastError,
    SetLastError,
    CreateEvent,
    CloseEvent,
    SetEvent,
    ResetEvent,
    WaitEvents,
    Htonl,
    Htons,
    Ntohl,
    Ntohs,
    PtonA,
    PtonW,
    NtopA,
    NtopW,
    Socket,
    SocketA,
    SocketW,
}

pub(crate) fn ordinal(name: &str) -> Option<&'static str> {
    match name.strip_prefix('#')?.parse::<u16>().ok()? {
        8 => Some("htonl"),
        9 => Some("htons"),
        14 => Some("ntohl"),
        15 => Some("ntohs"),
        23 => Some("socket"),
        111 => Some("WSAGetLastError"),
        112 => Some("WSASetLastError"),
        115 => Some("WSAStartup"),
        116 => Some("WSACleanup"),
        _ => None,
    }
}

pub(crate) fn symbol<'a>(dll: &str, name: &'a str) -> &'a str {
    if dll.eq_ignore_ascii_case("ws2_32.dll") {
        ordinal(name).unwrap_or(name)
    } else {
        name
    }
}

const fn declaration(
    id: WinsockApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let pe32_abi = matches!(
        id,
        WinsockApi::Startup | WinsockApi::Cleanup | WinsockApi::Socket
    );
    Contract::new(
        ApiId::Winsock(id), name, parameters, returns,
        "Win64 ws2_32 static/dynamic provider and existing ordinal aliases, process startup references, thread-local LastError carrier, checked WSADATA/address outputs, shared kernel events and process-controlled waits; no network socket backend; PE32 retains only existing Startup/Cleanup/socket ABI-only names and leaves ordinal resolution unchanged",
        [ArchitecturePlan { dlls: if pe32_abi { &["ws2_32.dll"] } else { &[] }, execution: if pe32_abi { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Absent } },
         ArchitecturePlan { dlls: &["ws2_32.dll"], execution: ExecutionPolicy::Typed }],
    )
}

use Parameter::{Pointer as P, Uint16 as W, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as R, Uint32 as U, Void as V};
pub static WINSOCK_CONTRACTS: [Contract; 20] = [
    declaration(WinsockApi::Startup, "WSAStartup", &[W, P], U),
    declaration(WinsockApi::Cleanup, "WSACleanup", &[], U),
    declaration(WinsockApi::GetLastError, "WSAGetLastError", &[], U),
    declaration(WinsockApi::SetLastError, "WSASetLastError", &[D], V),
    declaration(WinsockApi::CreateEvent, "WSACreateEvent", &[], R),
    declaration(WinsockApi::CloseEvent, "WSACloseEvent", &[P], B),
    declaration(WinsockApi::SetEvent, "WSASetEvent", &[P], B),
    declaration(WinsockApi::ResetEvent, "WSAResetEvent", &[P], B),
    declaration(
        WinsockApi::WaitEvents,
        "WSAWaitForMultipleEvents",
        &[D, P, D, D, D],
        U,
    ),
    declaration(WinsockApi::Htonl, "htonl", &[D], U),
    declaration(WinsockApi::Htons, "htons", &[D], U),
    declaration(WinsockApi::Ntohl, "ntohl", &[D], U),
    declaration(WinsockApi::Ntohs, "ntohs", &[D], U),
    declaration(WinsockApi::PtonA, "inet_pton", &[D, P, P], U),
    declaration(WinsockApi::PtonW, "InetPtonW", &[D, P, P], U),
    declaration(WinsockApi::NtopA, "inet_ntop", &[D, P, P, P], R),
    declaration(WinsockApi::NtopW, "InetNtopW", &[D, P, P, P], R),
    declaration(WinsockApi::Socket, "socket", &[D, D, D], R),
    declaration(WinsockApi::SocketA, "WSASocketA", &[D, D, D, P, D, D], R),
    declaration(WinsockApi::SocketW, "WSASocketW", &[D, D, D, P, D, D], R),
];
