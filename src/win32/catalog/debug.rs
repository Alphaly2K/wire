use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugApi {
    StringA,
    StringW,
    Break,
}

const fn declaration(id: DebugApi, name: &'static str) -> Contract {
    Contract::new(
        ApiId::Debug(id),
        name,
        &[Parameter::Pointer],
        ReturnConvention::Void,
        "optional null-terminated debug string; debugger transport is runtime-owned",
        [ArchitecturePlan {
            dlls: &[],
            execution: ExecutionPolicy::Absent,
        }; 2],
    )
}

pub static DEBUG_CONTRACTS: [Contract; 3] = [
    declaration(DebugApi::StringA, "OutputDebugStringA"),
    declaration(DebugApi::StringW, "OutputDebugStringW"),
    Contract::new(
        ApiId::Debug(DebugApi::Break),
        "DebugBreak",
        &[],
        ReturnConvention::Void,
        "raises a continuable breakpoint at the logical debug gateway",
        [ArchitecturePlan {
            dlls: &[],
            execution: ExecutionPolicy::Absent,
        }; 2],
    ),
];
