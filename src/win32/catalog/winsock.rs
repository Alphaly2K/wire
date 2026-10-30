//! Winsock contracts and provider-owned ordinal identities; no transport backend.
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
    Accept,
    Bind,
    Close,
    Connect,
    PeerName,
    SocketName,
    SocketOpt,
    Ioctl,
    Listen,
    Receive,
    ReceiveFrom,
    Select,
    Send,
    SendTo,
    SetSocketOpt,
    Shutdown,
    Address,
    AddressText,
    HostByAddress,
    HostByName,
    ProtoByName,
    ProtoByNumber,
    ServiceByName,
    ServiceByPort,
    HostName,
    AsyncSelect,
    AsyncHostByAddress,
    AsyncHostByName,
    AsyncProtoByNumber,
    AsyncProtoByName,
    AsyncServiceByPort,
    AsyncServiceByName,
    CancelAsync,
    SetBlockingHook,
    UnhookBlocking,
    CancelBlocking,
    IsBlocking,
    FdIsSet,
    EventSelect,
    NetworkEvents,
    IoctlEx,
    SendDisconnect,
    ReceiveDisconnect,
    ReceiveEx,
    ReceiveFromEx,
    SendEx,
    SendToEx,
    AddrInfoA,
    AddrInfoW,
    FreeAddrInfoA,
    FreeAddrInfoW,
    AddrInfoExW,
    FreeAddrInfoExW,
    CancelAddrInfo,
    NameInfoA,
    NameInfoW,
    HostNameW,
    AddrInfoResult,
}

pub(crate) fn ordinal(name: &str) -> Option<&'static str> {
    let ordinal = name.strip_prefix('#')?.parse::<u16>().ok()?;
    include_str!("../../../third_party/wine-10.0/abi/ws2_32.spec")
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.next()?.parse::<u16>().ok()? != ordinal || fields.next()? != "stdcall" {
                return None;
            }
            fields.next()?.split('(').next()
        })
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
        "Historical catalog plans retain the original architecture policies; current runtime admits both ABIs through ws2_32 with provider-owned ordinal aliases, process startup references, thread-local LastError carrier, checked WSADATA/address outputs, shared kernel events and process-controlled waits; no network socket backend",
        [ArchitecturePlan { dlls: if pe32_abi { &["ws2_32.dll"] } else { &[] }, execution: if pe32_abi { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Absent } },
         ArchitecturePlan { dlls: &["ws2_32.dll"], execution: ExecutionPolicy::Typed }],
    )
}

use Parameter::{Pointer as P, Uint16 as W, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as R, Uint32 as U, Void as V};
const fn added(
    id: WinsockApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(ApiId::Winsock(id), name, parameters, returns,
        "Both ABIs share classic ordinal aliases, address values, per-thread owned text/host records, numeric/local name resolution and fd_set membership; no transport, external DNS, async or blocking callbacks; checked inputs/outputs, full SOCKET/pointer widths and TEB error carrier",
        [ArchitecturePlan { dlls: &["ws2_32.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2])
}
pub static WINSOCK_CONTRACTS: [Contract; 78] = [
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
    added(WinsockApi::Accept, "accept", &[P, P, P], R),
    added(WinsockApi::Bind, "bind", &[P, P, D], U),
    added(WinsockApi::Close, "closesocket", &[P], U),
    added(WinsockApi::Connect, "connect", &[P, P, D], U),
    added(WinsockApi::PeerName, "getpeername", &[P, P, P], U),
    added(WinsockApi::SocketName, "getsockname", &[P, P, P], U),
    added(WinsockApi::SocketOpt, "getsockopt", &[P, D, D, P, P], U),
    added(WinsockApi::Ioctl, "ioctlsocket", &[P, D, P], U),
    added(WinsockApi::Listen, "listen", &[P, D], U),
    added(WinsockApi::Receive, "recv", &[P, P, D, D], U),
    added(WinsockApi::ReceiveFrom, "recvfrom", &[P, P, D, D, P, P], U),
    added(WinsockApi::Select, "select", &[D, P, P, P, P], U),
    added(WinsockApi::Send, "send", &[P, P, D, D], U),
    added(WinsockApi::SendTo, "sendto", &[P, P, D, D, P, D], U),
    added(WinsockApi::SetSocketOpt, "setsockopt", &[P, D, D, P, D], U),
    added(WinsockApi::Shutdown, "shutdown", &[P, D], U),
    added(WinsockApi::Address, "inet_addr", &[P], U),
    added(WinsockApi::AddressText, "inet_ntoa", &[D], R),
    added(WinsockApi::HostByAddress, "gethostbyaddr", &[P, D, D], R),
    added(WinsockApi::HostByName, "gethostbyname", &[P], R),
    added(WinsockApi::ProtoByName, "getprotobyname", &[P], R),
    added(WinsockApi::ProtoByNumber, "getprotobynumber", &[D], R),
    added(WinsockApi::ServiceByName, "getservbyname", &[P, P], R),
    added(WinsockApi::ServiceByPort, "getservbyport", &[D, P], R),
    added(WinsockApi::HostName, "gethostname", &[P, D], U),
    added(WinsockApi::AsyncSelect, "WSAAsyncSelect", &[P, P, D, D], U),
    added(
        WinsockApi::AsyncHostByAddress,
        "WSAAsyncGetHostByAddr",
        &[P, D, P, D, D, P, D],
        R,
    ),
    added(
        WinsockApi::AsyncHostByName,
        "WSAAsyncGetHostByName",
        &[P, D, P, P, D],
        R,
    ),
    added(
        WinsockApi::AsyncProtoByNumber,
        "WSAAsyncGetProtoByNumber",
        &[P, D, D, P, D],
        R,
    ),
    added(
        WinsockApi::AsyncProtoByName,
        "WSAAsyncGetProtoByName",
        &[P, D, P, P, D],
        R,
    ),
    added(
        WinsockApi::AsyncServiceByPort,
        "WSAAsyncGetServByPort",
        &[P, D, D, P, P, D],
        R,
    ),
    added(
        WinsockApi::AsyncServiceByName,
        "WSAAsyncGetServByName",
        &[P, D, P, P, P, D],
        R,
    ),
    added(WinsockApi::CancelAsync, "WSACancelAsyncRequest", &[P], U),
    added(WinsockApi::SetBlockingHook, "WSASetBlockingHook", &[P], R),
    added(WinsockApi::UnhookBlocking, "WSAUnhookBlockingHook", &[], U),
    added(WinsockApi::CancelBlocking, "WSACancelBlockingCall", &[], U),
    added(WinsockApi::IsBlocking, "WSAIsBlocking", &[], U),
    added(WinsockApi::FdIsSet, "__WSAFDIsSet", &[P, P], U),
    added(WinsockApi::EventSelect, "WSAEventSelect", &[P, P, D], U),
    added(
        WinsockApi::NetworkEvents,
        "WSAEnumNetworkEvents",
        &[P, P, P],
        U,
    ),
    added(
        WinsockApi::IoctlEx,
        "WSAIoctl",
        &[P, D, P, D, P, D, P, P, P],
        U,
    ),
    added(WinsockApi::SendDisconnect, "WSASendDisconnect", &[P, P], U),
    added(
        WinsockApi::ReceiveDisconnect,
        "WSARecvDisconnect",
        &[P, P],
        U,
    ),
    added(WinsockApi::ReceiveEx, "WSARecv", &[P, P, D, P, P, P, P], U),
    added(
        WinsockApi::ReceiveFromEx,
        "WSARecvFrom",
        &[P, P, D, P, P, P, P, P, P],
        U,
    ),
    added(WinsockApi::SendEx, "WSASend", &[P, P, D, P, D, P, P], U),
    added(
        WinsockApi::SendToEx,
        "WSASendTo",
        &[P, P, D, P, D, P, D, P, P],
        U,
    ),
    added(WinsockApi::AddrInfoA, "getaddrinfo", &[P, P, P, P], U),
    added(WinsockApi::AddrInfoW, "GetAddrInfoW", &[P, P, P, P], U),
    added(WinsockApi::FreeAddrInfoA, "freeaddrinfo", &[P], V),
    added(WinsockApi::FreeAddrInfoW, "FreeAddrInfoW", &[P], V),
    added(
        WinsockApi::AddrInfoExW,
        "GetAddrInfoExW",
        &[P, P, D, P, P, P, P, P, P, P],
        U,
    ),
    added(WinsockApi::FreeAddrInfoExW, "FreeAddrInfoExW", &[P], V),
    added(WinsockApi::CancelAddrInfo, "GetAddrInfoExCancel", &[P], U),
    added(
        WinsockApi::NameInfoA,
        "getnameinfo",
        &[P, D, P, D, P, D, D],
        U,
    ),
    added(
        WinsockApi::NameInfoW,
        "GetNameInfoW",
        &[P, D, P, D, P, D, D],
        U,
    ),
    added(WinsockApi::HostNameW, "GetHostNameW", &[P, D], U),
    added(
        WinsockApi::AddrInfoResult,
        "GetAddrInfoExOverlappedResult",
        &[P],
        U,
    ),
];
