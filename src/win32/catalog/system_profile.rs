use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemProfileApi {
    SystemInfo,
    MemoryStatus,
    MemoryStatusEx,
    ProcessorTopology,
    DriveTypeA,
    DriveTypeW,
    PowerStatus,
    DiskSpaceA,
    DiskSpaceW,
    ProcessorNumber,
    LogicalDrivesA,
    LogicalDrivesW,
    VolumeA,
    VolumeW,
    FlushWrites,
    LanguageNameA,
    LanguageNameW,
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

pub static SYSTEM_PROFILE_CONTRACTS: [Contract; 18] = [
    support(SystemProfileApi::LanguageNameA,"VerLanguageNameA",&[Parameter::Uint32,Parameter::Pointer,Parameter::Uint32]),
    support(SystemProfileApi::LanguageNameW,"VerLanguageNameW",&[Parameter::Uint32,Parameter::Pointer,Parameter::Uint32]),
    support(SystemProfileApi::FlushWrites,"FlushProcessWriteBuffers",&[]),
    support(SystemProfileApi::ProcessorNumber,"GetCurrentProcessorNumberEx",&[Parameter::Pointer]),
    support(SystemProfileApi::LogicalDrivesA,"GetLogicalDriveStringsA",&[Parameter::Uint32,Parameter::Pointer]),
    support(SystemProfileApi::LogicalDrivesW,"GetLogicalDriveStringsW",&[Parameter::Uint32,Parameter::Pointer]),
    support(SystemProfileApi::VolumeA,"GetVolumeInformationA",&[Parameter::Pointer,Parameter::Pointer,Parameter::Uint32,Parameter::Pointer,Parameter::Pointer,Parameter::Pointer,Parameter::Pointer,Parameter::Uint32]),
    support(SystemProfileApi::VolumeW,"GetVolumeInformationW",&[Parameter::Pointer,Parameter::Pointer,Parameter::Uint32,Parameter::Pointer,Parameter::Pointer,Parameter::Pointer,Parameter::Pointer,Parameter::Uint32]),
    disk_space(SystemProfileApi::DiskSpaceA, "GetDiskFreeSpaceExA"),
    disk_space(SystemProfileApi::DiskSpaceW, "GetDiskFreeSpaceExW"),
    Contract::new(
        ApiId::SystemProfile(SystemProfileApi::PowerStatus), "GetSystemPowerStatus",
        &[Parameter::Pointer], ReturnConvention::BoolLastError,
        "12-byte virtual desktop profile: AC online, no battery, unknown battery percent/lifetimes, battery saver off; checked full output, success preserves LastError",
        [ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }; 2],
    ),
    drive_type(SystemProfileApi::DriveTypeA, "GetDriveTypeA"),
    drive_type(SystemProfileApi::DriveTypeW, "GetDriveTypeW"),
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
    Contract::new(
        ApiId::SystemProfile(SystemProfileApi::ProcessorTopology), "GetLogicalProcessorInformationEx",
        &[Parameter::Uint32, Parameter::Pointer, Parameter::Pointer], ReturnConvention::BoolLastError,
        "one virtual guest CPU, NUMA node and group, no advertised caches; existing Win64 relation records and checked capacity/output/error order; PE32 kernel32 Wine ABI-only, excluded from the public system facade",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::LegacyUnimplemented },
         ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }],
    ),
];

const fn disk_space(id: SystemProfileApi, name: &'static str) -> Contract {
    Contract::new(ApiId::SystemProfile(id), name,
        &[Parameter::Pointer; 4], ReturnConvention::BoolLastError,
        "Directory is resolved through the existing VFS; no logical disk capacity/quota model: valid directories fail ERROR_NOT_SUPPORTED with unchanged optional 64-bit outputs",
        [ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2])
}

const fn drive_type(id: SystemProfileApi, name: &'static str) -> Contract {
    Contract::new(
        ApiId::SystemProfile(id), name, &[Parameter::Pointer], ReturnConvention::Uint32,
        "virtual C volume is fixed; NULL uses current-directory root, other input requires trailing slash and an existing VFS directory; unavailable roots return DRIVE_NO_ROOT_DIR; checked A/W input, preserves LastError",
        [ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }; 2],
    )
}

const fn support(
    id: SystemProfileApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(ApiId::SystemProfile(id),name,parameters,match id {SystemProfileApi::ProcessorNumber|SystemProfileApi::FlushWrites=>ReturnConvention::Void,SystemProfileApi::LogicalDrivesA|SystemProfileApi::LogicalDrivesW|SystemProfileApi::LanguageNameA|SystemProfileApi::LanguageNameW=>ReturnConvention::Uint32,_=>ReturnConvention::BoolLastError},"One virtual processor/group and C drive; checked VFS volume metadata and output transactions, preserved LastError",[ArchitecturePlan {dlls:&["kernel32.dll","kernelbase.dll"],execution:ExecutionPolicy::LegacyUnimplemented};2])
}
