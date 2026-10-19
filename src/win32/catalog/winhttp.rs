use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinHttpApi {
    UserProxy,
    DefaultProxy,
    ProxyForUrl,
    Open,
    Close,
}

const fn declaration(
    id: WinHttpApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(
        ApiId::WinHttp(id), name, parameters,
        if matches!(id, WinHttpApi::Open) { ReturnConvention::Pointer } else { ReturnConvention::BoolLastError },
        "PE32 absent/unknown ABI; Win64 winhttp.dll only for static/dynamic export; no IE profile or HTTP transport, user proxy preflights 32 bytes then fails error 2 without output, default direct proxy writes 24 bytes and preserves LastError, NULL profile output error 87, Open error 50, no session handles and proxy/close error 6, full pointers and DWORD Open access/flags, original output/fault/error order",
        [ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent },
         ArchitecturePlan { dlls: &["winhttp.dll"], execution: ExecutionPolicy::Typed }],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
pub static WINHTTP_CONTRACTS: [Contract; 5] = [
    declaration(
        WinHttpApi::UserProxy,
        "WinHttpGetIEProxyConfigForCurrentUser",
        &[P],
    ),
    declaration(
        WinHttpApi::DefaultProxy,
        "WinHttpGetDefaultProxyConfiguration",
        &[P],
    ),
    declaration(
        WinHttpApi::ProxyForUrl,
        "WinHttpGetProxyForUrl",
        &[P, P, P, P],
    ),
    declaration(WinHttpApi::Open, "WinHttpOpen", &[P, D, P, P, D]),
    declaration(WinHttpApi::Close, "WinHttpCloseHandle", &[P]),
];
