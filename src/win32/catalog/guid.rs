use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuidApi {
    StringFromGuid2,
    ClsidFromString,
    IidFromString,
    Create,
    AllocateClsidText,
    AllocateIidText,
}

const fn resource_declaration(
    id: GuidApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(
        ApiId::Guid(id), name, parameters, ReturnConvention::Hresult,
        "existing random-v4 GUID and allocated canonical text; PE32 process-heap sharing, output zero before allocation, HRESULT also writes LastError; Win64 checked full pointers and task membership, output published after allocation, saved return frame; original null/error/fault order and entropy failure HRESULT differ by architecture",
        [
            ArchitecturePlan { dlls: &["ole32.dll"], execution: ExecutionPolicy::Typed },
            ArchitecturePlan { dlls: &["ole32.dll", "combase.dll"], execution: ExecutionPolicy::Typed },
        ],
    )
}

pub static GUID_CONTRACTS: [Contract; 6] = [
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
    resource_declaration(GuidApi::Create, "CoCreateGuid", &[Parameter::Pointer]),
    resource_declaration(
        GuidApi::AllocateClsidText,
        "StringFromCLSID",
        &[Parameter::Pointer, Parameter::Pointer],
    ),
    resource_declaration(
        GuidApi::AllocateIidText,
        "StringFromIID",
        &[Parameter::Pointer, Parameter::Pointer],
    ),
];
