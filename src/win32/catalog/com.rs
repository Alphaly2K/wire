use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComApi {
    Initialize,
    InitializeEx,
    Uninitialize,
    WinrtInitialize,
    WinrtUninitialize,
    CreateInstance,
    TaskAlloc,
    TaskFree,
    OleInitialize,
    OleUninitialize,
    PropertyClear,
    PropertyCopy,
    FreeMarshaler,
    ProxyBlanket,
    AgileReference,
    TaskRealloc,
}

const COM_PROVIDERS: &[&str] = &[
    "ole32.dll",
    "combase.dll",
    "api-ms-win-core-winrt-l1-1-0.dll",
];
const COM_IMPORT_PROVIDERS: &[&str] = &[
    "kernel32.dll",
    "kernelbase.dll",
    "ole32.dll",
    "combase.dll",
    "api-ms-win-core-winrt-l1-1-0.dll",
];

const fn declaration(
    id: ComApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    if matches!(
        id,
        ComApi::PropertyClear
            | ComApi::PropertyCopy
            | ComApi::FreeMarshaler
            | ComApi::ProxyBlanket
            | ComApi::AgileReference
    ) {
        return Contract::new(ApiId::Com(id), name, parameters, returns,
            "Both ABIs use checked PROPVARIANT records and existing task/BSTR/SAFEARRAY owners; copy never clears the destination, clear zeroes the full record after ownership preflight. Unsupported marshaling/proxy/agile services return explicit HRESULT failures and no interface object.",
            [ArchitecturePlan { dlls: &["ole32.dll", "combase.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2]);
    }
    let winrt = matches!(id, ComApi::WinrtInitialize | ComApi::WinrtUninitialize);
    let ole = matches!(id, ComApi::OleInitialize | ComApi::OleUninitialize);
    let x86_dlls: &[&str] = if winrt { &[] } else { &["ole32.dll"] };
    Contract::new(
        ApiId::Com(id), name, parameters, returns,
        "existing per-thread COM/WinRT references; PE32 HRESULT also writes LastError, process heap task sharing, WMI-only class factory and Ole ABI-only; Win64 separate COM/WinRT counts, saved frame, full-width reserved/SIZE_T/pointers and DWORD flags, checked task membership, explicit missing factory; static kernel imports allowed but default dynamic lookup excluded",
        [
            ArchitecturePlan { dlls: x86_dlls, execution: if winrt { ExecutionPolicy::Absent } else if ole { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Typed } },
            ArchitecturePlan { dlls: if ole { &[] } else { COM_IMPORT_PROVIDERS }, execution: if ole { ExecutionPolicy::Absent } else { ExecutionPolicy::Typed } },
        ],
    ).with_lookup_dlls([x86_dlls, if ole { &[] } else { COM_PROVIDERS }])
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{Hresult as H, Pointer as R, Void as V};
pub static COM_CONTRACTS: [Contract; 16] = [
    Contract::new(ApiId::Com(ComApi::TaskRealloc), "CoTaskMemRealloc", &[P,P], R, "Existing COM task owner and realloc; NULL allocates, non-NULL zero frees, failure preserves original block and LastError", [ArchitecturePlan { dlls: &["ole32.dll","combase.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2]),
    declaration(ComApi::Initialize, "CoInitialize", &[P], H),
    declaration(ComApi::InitializeEx, "CoInitializeEx", &[P, D], H),
    declaration(ComApi::Uninitialize, "CoUninitialize", &[], V),
    declaration(ComApi::WinrtInitialize, "RoInitialize", &[D], H),
    declaration(ComApi::WinrtUninitialize, "RoUninitialize", &[], V),
    declaration(
        ComApi::CreateInstance,
        "CoCreateInstance",
        &[P, P, D, P, P],
        H,
    ),
    declaration(ComApi::TaskAlloc, "CoTaskMemAlloc", &[P], R),
    declaration(ComApi::TaskFree, "CoTaskMemFree", &[P], V),
    declaration(ComApi::OleInitialize, "OleInitialize", &[P], H),
    declaration(ComApi::OleUninitialize, "OleUninitialize", &[], V),
    declaration(ComApi::PropertyClear, "PropVariantClear", &[P], H),
    declaration(ComApi::PropertyCopy, "PropVariantCopy", &[P, P], H),
    declaration(
        ComApi::FreeMarshaler,
        "CoCreateFreeThreadedMarshaler",
        &[P, P],
        H,
    ),
    declaration(
        ComApi::ProxyBlanket,
        "CoSetProxyBlanket",
        &[P, D, D, P, D, D, P, D],
        H,
    ),
    declaration(
        ComApi::AgileReference,
        "RoGetAgileReference",
        &[D, P, P, P],
        H,
    ),
];
