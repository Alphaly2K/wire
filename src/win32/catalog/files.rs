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
    GetCurrentDirectoryA,
    GetCurrentDirectoryW,
    SetCurrentDirectoryA,
    SetCurrentDirectoryW,
    GetFullPathNameA,
    GetFullPathNameW,
    GetFileAttributesA,
    GetFileAttributesW,
    SetFileAttributesA,
    SetFileAttributesW,
}

impl FileApi {
    pub(crate) fn wide(self) -> bool {
        matches!(
            self,
            Self::CreateFileW
                | Self::GetCurrentDirectoryW
                | Self::SetCurrentDirectoryW
                | Self::GetFullPathNameW
                | Self::GetFileAttributesW
                | Self::SetFileAttributesW
        )
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
        "confined synchronous file/path operations; existing sharing/access and transfer limits; architecture-specific output fault and LastError order; Win64 read/write retain endpoint-specific completion without scheduler unblock; PE32 retains split 64-bit stack arguments",
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
use ReturnConvention::CharacterCount as C;
use ReturnConvention::{BoolLastError as B, Pointer as R, Uint32 as U};

pub static FILE_CONTRACTS: [Contract; 21] = [
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
    declaration(
        FileApi::GetCurrentDirectoryA,
        "GetCurrentDirectoryA",
        &[D, P],
        C,
        true,
    ),
    declaration(
        FileApi::GetCurrentDirectoryW,
        "GetCurrentDirectoryW",
        &[D, P],
        C,
        true,
    ),
    declaration(
        FileApi::SetCurrentDirectoryA,
        "SetCurrentDirectoryA",
        &[P],
        B,
        true,
    ),
    declaration(
        FileApi::SetCurrentDirectoryW,
        "SetCurrentDirectoryW",
        &[P],
        B,
        true,
    ),
    declaration(
        FileApi::GetFullPathNameA,
        "GetFullPathNameA",
        &[P, D, P, P],
        C,
        false,
    ),
    declaration(
        FileApi::GetFullPathNameW,
        "GetFullPathNameW",
        &[P, D, P, P],
        C,
        false,
    ),
    declaration(
        FileApi::GetFileAttributesA,
        "GetFileAttributesA",
        &[P],
        U,
        true,
    ),
    declaration(
        FileApi::GetFileAttributesW,
        "GetFileAttributesW",
        &[P],
        U,
        true,
    ),
    declaration(
        FileApi::SetFileAttributesA,
        "SetFileAttributesA",
        &[P, D],
        B,
        true,
    ),
    declaration(
        FileApi::SetFileAttributesW,
        "SetFileAttributesW",
        &[P, D],
        B,
        true,
    ),
];
