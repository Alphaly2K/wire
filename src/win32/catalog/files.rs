use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileApi {
    CreateFileA,
    CreateFileW,
    ReadFile,
    WriteFile,
    GetFileSize,
    GetFileSizeEx,
    SetFilePointer,
    SetFilePointerEx,
    SetEndOfFile,
    FlushFileBuffers,
    GetFileType,
}

impl FileApi {
    pub(crate) fn wide(self) -> bool {
        self == Self::CreateFileW
    }
}

const fn declaration(
    id: FileApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
    win64: bool,
) -> Contract {
    Contract::new(
        ApiId::File(id), name, parameters, returns,
        "confined synchronous file I/O; existing sharing/access and transfer limits; architecture-specific seek/output fault and LastError order; Win64 read/write retain endpoint-specific completion without scheduler unblock; PE32 retains split 64-bit stack arguments",
        [
            ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::Typed },
            ArchitecturePlan {
                dlls: if win64 { &["kernel32.dll", "kernelbase.dll"] } else { &[] },
                execution: if win64 { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent },
            },
        ],
    )
}

use Parameter::{Int32 as I, Int64 as Q, Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Pointer as R, Uint32 as U};

pub static FILE_CONTRACTS: [Contract; 11] = [
    declaration(
        FileApi::CreateFileA,
        "CreateFileA",
        &[P, D, D, P, D, D, P],
        R,
        true,
    ),
    declaration(
        FileApi::CreateFileW,
        "CreateFileW",
        &[P, D, D, P, D, D, P],
        R,
        true,
    ),
    declaration(FileApi::ReadFile, "ReadFile", &[P, P, D, P, P], B, true),
    declaration(FileApi::WriteFile, "WriteFile", &[P, P, D, P, P], B, true),
    declaration(FileApi::GetFileSize, "GetFileSize", &[P, P], U, true),
    declaration(FileApi::GetFileSizeEx, "GetFileSizeEx", &[P, P], B, true),
    declaration(
        FileApi::SetFilePointer,
        "SetFilePointer",
        &[P, I, P, D],
        U,
        false,
    ),
    declaration(
        FileApi::SetFilePointerEx,
        "SetFilePointerEx",
        &[P, Q, P, D],
        B,
        true,
    ),
    declaration(FileApi::SetEndOfFile, "SetEndOfFile", &[P], B, false),
    declaration(FileApi::FlushFileBuffers, "FlushFileBuffers", &[P], B, true),
    declaration(FileApi::GetFileType, "GetFileType", &[P], U, true),
];
