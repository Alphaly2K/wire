use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemProfileApi {
    SystemInfo,
    MemoryStatus,
    MemoryStatusEx,
}

const fn declaration(
    id: SystemProfileApi,
    name: &'static str,
    returns: ReturnConvention,
    win64: bool,
) -> Contract {
    Contract::new(
        ApiId::SystemProfile(id), name, &[Parameter::Pointer], returns,
        "architecture-specific virtual hardware and memory accounting; 36/48-byte SYSTEM_INFO, 32/64-byte memory status, Ex requires length 64; PE32 output errors become error 87 and retain unsaved return-slot alias, Win64 checked faults preserve CPU/API count and saved frame; public PE32 system facade retains exact DLL matching",
        [
            ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::Typed },
            ArchitecturePlan {
                dlls: if win64 { &["kernel32.dll", "kernelbase.dll"] } else { &[] },
                execution: if win64 { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent },
            },
        ],
    )
}

pub static SYSTEM_PROFILE_CONTRACTS: [Contract; 4] = [
    declaration(
        SystemProfileApi::SystemInfo,
        "GetSystemInfo",
        ReturnConvention::Void,
        true,
    ),
    declaration(
        SystemProfileApi::SystemInfo,
        "GetNativeSystemInfo",
        ReturnConvention::Void,
        true,
    ),
    declaration(
        SystemProfileApi::MemoryStatus,
        "GlobalMemoryStatus",
        ReturnConvention::Void,
        false,
    ),
    declaration(
        SystemProfileApi::MemoryStatusEx,
        "GlobalMemoryStatusEx",
        ReturnConvention::BoolLastError,
        true,
    ),
];
