use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VersionApi {
    GetVersion,
    GetVersionExA,
    GetVersionExW,
    RtlGetVersion,
    VerifyVersionInfoA,
    VerifyVersionInfoW,
    RtlVerifyVersionInfo,
    VerSetConditionMask,
}

const fn declaration(
    id: VersionApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
    x86: ArchitecturePlan,
    x64: ArchitecturePlan,
) -> Contract {
    Contract::new(
        ApiId::Version(id), name, parameters, returns,
        "guest profile; PE32 query keeps cross-encoding sizes and legacy LastError/NT-status behavior; verify/mask remain ABI-only on PE32; Win64 preserves historical kernel Rtl exports and raw BYTE trace carrier",
        [x86, x64],
    )
}
const QUERY32: ArchitecturePlan = ArchitecturePlan {
    dlls: &["kernel32.dll"],
    execution: ExecutionPolicy::Typed,
};
const KERNEL64: ArchitecturePlan = ArchitecturePlan {
    dlls: &["kernel32.dll", "kernelbase.dll"],
    execution: ExecutionPolicy::Typed,
};
const RTL64: ArchitecturePlan = ArchitecturePlan {
    dlls: &["kernel32.dll", "kernelbase.dll", "ntdll.dll"],
    execution: ExecutionPolicy::Typed,
};
const ABI32: ArchitecturePlan = ArchitecturePlan {
    dlls: &["kernel32.dll"],
    execution: ExecutionPolicy::LegacyUnimplemented,
};
const ABSENT: ArchitecturePlan = ArchitecturePlan {
    dlls: &[],
    execution: ExecutionPolicy::Absent,
};

pub static VERSION_CONTRACTS: [Contract; 8] = [
    declaration(
        VersionApi::GetVersion,
        "GetVersion",
        &[],
        ReturnConvention::Uint32,
        QUERY32,
        KERNEL64,
    ),
    declaration(
        VersionApi::GetVersionExA,
        "GetVersionExA",
        &[Parameter::Pointer],
        ReturnConvention::BoolLastError,
        QUERY32,
        KERNEL64,
    ),
    declaration(
        VersionApi::GetVersionExW,
        "GetVersionExW",
        &[Parameter::Pointer],
        ReturnConvention::BoolLastError,
        QUERY32,
        KERNEL64,
    ),
    declaration(
        VersionApi::RtlGetVersion,
        "RtlGetVersion",
        &[Parameter::Pointer],
        ReturnConvention::Ntstatus,
        ArchitecturePlan {
            dlls: &["ntdll.dll"],
            execution: ExecutionPolicy::Typed,
        },
        RTL64,
    ),
    declaration(
        VersionApi::VerifyVersionInfoA,
        "VerifyVersionInfoA",
        &[Parameter::Pointer, Parameter::Uint32, Parameter::Uint64],
        ReturnConvention::BoolLastError,
        ABI32,
        ABSENT,
    ),
    declaration(
        VersionApi::VerifyVersionInfoW,
        "VerifyVersionInfoW",
        &[Parameter::Pointer, Parameter::Uint32, Parameter::Uint64],
        ReturnConvention::BoolLastError,
        ABI32,
        KERNEL64,
    ),
    declaration(
        VersionApi::RtlVerifyVersionInfo,
        "RtlVerifyVersionInfo",
        &[Parameter::Pointer, Parameter::Uint32, Parameter::Uint64],
        ReturnConvention::Ntstatus,
        ABSENT,
        RTL64,
    ),
    declaration(
        VersionApi::VerSetConditionMask,
        "VerSetConditionMask",
        &[Parameter::Uint64, Parameter::Uint32, Parameter::Uint8],
        ReturnConvention::Uint64,
        ABI32,
        RTL64,
    ),
];
