use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafeArrayApi {
    Create,
    CreateVector,
    Destroy,
    Copy,
    Dimensions,
    ElementSize,
    LowerBound,
    UpperBound,
    Lock,
    Unlock,
    Access,
    Unaccess,
    Put,
    Get,
    Pointer,
    Vartype,
    CreateEx,
    CreateVectorEx,
}

pub(crate) fn symbol<'a>(dll: &str, name: &'a str) -> &'a str {
    if !dll.eq_ignore_ascii_case("oleaut32.dll") {
        return name;
    }
    match name {
        "#15" => "SafeArrayCreate",
        "#16" => "SafeArrayDestroy",
        "#17" => "SafeArrayGetDim",
        "#18" => "SafeArrayGetElemsize",
        "#19" => "SafeArrayGetUBound",
        "#20" => "SafeArrayGetLBound",
        "#21" => "SafeArrayLock",
        "#22" => "SafeArrayUnlock",
        "#23" => "SafeArrayAccessData",
        "#24" => "SafeArrayUnaccessData",
        "#25" => "SafeArrayGetElement",
        "#26" => "SafeArrayPutElement",
        "#27" => "SafeArrayCopy",
        "#42" => "SafeArrayCreateEx",
        "#148" => "SafeArrayPtrOfIndex",
        "#411" => "SafeArrayCreateVector",
        "#77" => "SafeArrayGetVartype",
        "#43" => "SafeArrayCreateVectorEx",
        _ => name,
    }
}

const fn contract(
    id: SafeArrayApi,
    name: &'static str,
    args: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(ApiId::SafeArray(id), name, args, returns,
        "owned bounded SAFEARRAY descriptors/data; scalar, BSTR and VARIANT elements; complete ABI pointers, reversed descriptor bounds and deep ownership; interface/record/foreign descriptor operations remain unsupported; HRESULT operations preserve LastError",
        [ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }])
}
use Parameter::{Int32 as I, Pointer as P, Uint16 as W, Uint32 as D};
use ReturnConvention::{Pointer as R, Uint32 as U};
pub static SAFE_ARRAY_CONTRACTS: [Contract; 18] = [
    contract(SafeArrayApi::Create, "SafeArrayCreate", &[W, D, P], R),
    contract(
        SafeArrayApi::CreateVector,
        "SafeArrayCreateVector",
        &[W, I, D],
        R,
    ),
    contract(SafeArrayApi::Destroy, "SafeArrayDestroy", &[P], U),
    contract(SafeArrayApi::Copy, "SafeArrayCopy", &[P, P], U),
    contract(SafeArrayApi::Dimensions, "SafeArrayGetDim", &[P], U),
    contract(SafeArrayApi::ElementSize, "SafeArrayGetElemsize", &[P], U),
    contract(
        SafeArrayApi::LowerBound,
        "SafeArrayGetLBound",
        &[P, D, P],
        U,
    ),
    contract(
        SafeArrayApi::UpperBound,
        "SafeArrayGetUBound",
        &[P, D, P],
        U,
    ),
    contract(SafeArrayApi::Lock, "SafeArrayLock", &[P], U),
    contract(SafeArrayApi::Unlock, "SafeArrayUnlock", &[P], U),
    contract(SafeArrayApi::Access, "SafeArrayAccessData", &[P, P], U),
    contract(SafeArrayApi::Unaccess, "SafeArrayUnaccessData", &[P], U),
    contract(SafeArrayApi::Put, "SafeArrayPutElement", &[P, P, P], U),
    contract(SafeArrayApi::Get, "SafeArrayGetElement", &[P, P, P], U),
    contract(SafeArrayApi::Pointer, "SafeArrayPtrOfIndex", &[P, P, P], U),
    contract(SafeArrayApi::Vartype, "SafeArrayGetVartype", &[P, P], U),
    contract(
        SafeArrayApi::CreateEx,
        "SafeArrayCreateEx",
        &[W, D, P, P],
        R,
    ),
    contract(
        SafeArrayApi::CreateVectorEx,
        "SafeArrayCreateVectorEx",
        &[W, I, D, P],
        R,
    ),
];
