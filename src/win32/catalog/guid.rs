use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuidApi {
    StringFromGuid2,
    ClsidFromString,
    IidFromString,
}

pub static GUID_CONTRACTS: [Contract; 3] = [
    Contract::new(
        ApiId::Guid(GuidApi::StringFromGuid2),
        "StringFromGUID2",
        &[Parameter::Pointer, Parameter::Pointer, Parameter::Int32],
        ReturnConvention::CharacterCount,
        "canonical 38-character braced GUID plus NUL",
        [
            ArchitecturePlan {
                dlls: &["ole32.dll"],
                execution: ExecutionPolicy::Typed,
            },
            ArchitecturePlan {
                dlls: &["ole32.dll", "combase.dll"],
                execution: ExecutionPolicy::Typed,
            },
        ],
    ),
    Contract::new(
        ApiId::Guid(GuidApi::ClsidFromString),
        "CLSIDFromString",
        &[Parameter::Pointer, Parameter::Pointer],
        ReturnConvention::Hresult,
        "canonical GUID/null input; no activation-context or installed ProgIDs",
        [
            ArchitecturePlan {
                dlls: &["ole32.dll"],
                execution: ExecutionPolicy::Typed,
            },
            ArchitecturePlan {
                dlls: &["ole32.dll", "combase.dll"],
                execution: ExecutionPolicy::Typed,
            },
        ],
    ),
    Contract::new(
        ApiId::Guid(GuidApi::IidFromString),
        "IIDFromString",
        &[Parameter::Pointer, Parameter::Pointer],
        ReturnConvention::Hresult,
        "canonical GUID/null input; architecture-specific legacy HRESULT and fault behavior",
        [
            ArchitecturePlan {
                dlls: &["ole32.dll"],
                execution: ExecutionPolicy::Typed,
            },
            ArchitecturePlan {
                dlls: &["ole32.dll", "combase.dll"],
                execution: ExecutionPolicy::Typed,
            },
        ],
    ),
];
