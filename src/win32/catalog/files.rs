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
    GetFileAttributesExA,
    GetFileAttributesExW,
    GetFileInformationByHandle,
    GetFileInformationByHandleEx,
    GetFileTime,
    CreateDirectoryA,
    CreateDirectoryW,
    DeleteFileA,
    DeleteFileW,
    MoveFileA,
    MoveFileW,
    MoveFileExA,
    MoveFileExW,
    RemoveDirectoryA,
    RemoveDirectoryW,
    CopyFileA,
    CopyFileW,
    FindFirstFileA,
    FindFirstFileW,
    FindFirstFileExA,
    FindFirstFileExW,
    FindNextFileA,
    FindNextFileW,
    FindClose,
    CreateFileMappingA,
    CreateFileMappingW,
    OpenFileMappingA,
    OpenFileMappingW,
    MapViewOfFile,
    MapViewOfFileEx,
    UnmapViewOfFile,
    FlushViewOfFile,
    AreFileApisAnsi,
    CreatePipe,
    GetConsoleMode,
    WriteConsoleW,
    NtQueryDirectory,
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
                | Self::GetFileAttributesExW
                | Self::CreateDirectoryW
                | Self::DeleteFileW
                | Self::MoveFileW
                | Self::MoveFileExW
                | Self::RemoveDirectoryW
                | Self::CopyFileW
                | Self::FindFirstFileW
                | Self::FindFirstFileExW
                | Self::FindNextFileW
        )
    }

    pub(crate) fn is_search(self) -> bool {
        matches!(
            self,
            Self::FindFirstFileA
                | Self::FindFirstFileW
                | Self::FindFirstFileExA
                | Self::FindFirstFileExW
                | Self::FindNextFileA
                | Self::FindNextFileW
                | Self::FindClose
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

const fn metadata(id: FileApi, name: &'static str, parameters: &'static [Parameter]) -> Contract {
    Contract::new(
        ApiId::File(id),
        name,
        parameters,
        B,
        "PE32 known ABI with legacy unimplemented return; Win64 confined host metadata, supported information classes and directory cursor semantics; preserve output preflight, return-frame alias and LastError order",
        [
            ArchitecturePlan {
                dlls: &["kernel32.dll"],
                execution: ExecutionPolicy::LegacyUnimplemented,
            },
            ArchitecturePlan {
                dlls: &["kernel32.dll", "kernelbase.dll"],
                execution: ExecutionPolicy::Typed,
            },
        ],
    )
}

const fn endpoint(id: FileApi, name: &'static str, parameters: &'static [Parameter]) -> Contract {
    let mut contract = metadata(id, name, parameters);
    contract.limits = "PE32 kernel32 ABI-only; Win64 pipe output preflight, 24-byte security attributes, inherited endpoint flags and bounded allocation; redirected byte streams are not consoles, mode/write-console retain error 6 and unchanged outputs; original full-width handles and DWORD size/count";
    contract
}

const fn mutation(
    id: FileApi,
    name: &'static str,
    parameters: &'static [Parameter],
    win64: bool,
) -> Contract {
    Contract::new(
        ApiId::File(id),
        name,
        parameters,
        B,
        "PE32 known ABI with legacy unimplemented return; Win64 confined path mutations, existing flags/sharing/cwd checks and metadata update order; CopyFile remains absent on Win64",
        [
            ArchitecturePlan {
                dlls: &["kernel32.dll"],
                execution: ExecutionPolicy::LegacyUnimplemented,
            },
            ArchitecturePlan {
                dlls: if win64 { &["kernel32.dll", "kernelbase.dll"] } else { &[] },
                execution: if win64 { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent },
            },
        ],
    )
}

const fn search(
    id: FileApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::File(id), name, parameters, returns,
        "confined directory search; existing basic/extended information and flags subset; independent search handles; output before handle allocation or cursor advancement; architecture-specific fault/return-frame order and metadata limits",
        [
            ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::Typed },
            ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed },
        ],
    )
}

const fn section(
    id: FileApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
    win64: bool,
) -> Contract {
    Contract::new(
        ApiId::File(id), name, parameters, returns,
        "PE32 known ABI with legacy unimplemented return; Win64 unnamed read-only file-backed sections, retained backing/view lifetime and file-write coherence; preserve security/access/alignment checks, pointer widths and fault/LastError order; OpenFileMapping remains absent on Win64",
        [
            ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::LegacyUnimplemented },
            ArchitecturePlan {
                dlls: if win64 { &["kernel32.dll", "kernelbase.dll"] } else { &[] },
                execution: if win64 { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent },
            },
        ],
    )
}

pub static FILE_CONTRACTS: [Contract; 58] = [
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
        true,
    ),
    declaration(
        FileApi::SetFilePointerEx,
        "SetFilePointerEx",
        &[P, Q, P, D],
        B,
        true,
    ),
    declaration(FileApi::SetEndOfFile, "SetEndOfFile", &[P], B, true),
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
    metadata(
        FileApi::GetFileAttributesExA,
        "GetFileAttributesExA",
        &[P, D, P],
    ),
    metadata(
        FileApi::GetFileAttributesExW,
        "GetFileAttributesExW",
        &[P, D, P],
    ),
    metadata(
        FileApi::GetFileInformationByHandle,
        "GetFileInformationByHandle",
        &[P, P],
    ),
    metadata(
        FileApi::GetFileInformationByHandleEx,
        "GetFileInformationByHandleEx",
        &[P, D, P, D],
    ),
    metadata(FileApi::GetFileTime, "GetFileTime", &[P, P, P, P]),
    declaration(
        FileApi::CreateDirectoryA,
        "CreateDirectoryA",
        &[P, P],
        B,
        true,
    ),
    declaration(
        FileApi::CreateDirectoryW,
        "CreateDirectoryW",
        &[P, P],
        B,
        true,
    ),
    declaration(FileApi::DeleteFileA, "DeleteFileA", &[P], B, true),
    declaration(FileApi::DeleteFileW, "DeleteFileW", &[P], B, true),
    mutation(FileApi::MoveFileA, "MoveFileA", &[P, P], true),
    mutation(FileApi::MoveFileW, "MoveFileW", &[P, P], true),
    mutation(FileApi::MoveFileExA, "MoveFileExA", &[P, P, D], true),
    mutation(FileApi::MoveFileExW, "MoveFileExW", &[P, P, D], true),
    mutation(FileApi::RemoveDirectoryA, "RemoveDirectoryA", &[P], true),
    mutation(FileApi::RemoveDirectoryW, "RemoveDirectoryW", &[P], true),
    mutation(FileApi::CopyFileA, "CopyFileA", &[P, P, I], false),
    mutation(FileApi::CopyFileW, "CopyFileW", &[P, P, I], false),
    search(FileApi::FindFirstFileA, "FindFirstFileA", &[P, P], R),
    search(FileApi::FindFirstFileW, "FindFirstFileW", &[P, P], R),
    search(
        FileApi::FindFirstFileExA,
        "FindFirstFileExA",
        &[P, D, P, D, P, D],
        R,
    ),
    search(
        FileApi::FindFirstFileExW,
        "FindFirstFileExW",
        &[P, D, P, D, P, D],
        R,
    ),
    search(FileApi::FindNextFileA, "FindNextFileA", &[P, P], B),
    search(FileApi::FindNextFileW, "FindNextFileW", &[P, P], B),
    search(FileApi::FindClose, "FindClose", &[P], B),
    section(
        FileApi::CreateFileMappingA,
        "CreateFileMappingA",
        &[P, P, D, D, D, P],
        R,
        true,
    ),
    section(
        FileApi::CreateFileMappingW,
        "CreateFileMappingW",
        &[P, P, D, D, D, P],
        R,
        true,
    ),
    section(
        FileApi::OpenFileMappingA,
        "OpenFileMappingA",
        &[D, I, P],
        R,
        false,
    ),
    section(
        FileApi::OpenFileMappingW,
        "OpenFileMappingW",
        &[D, I, P],
        R,
        false,
    ),
    section(
        FileApi::MapViewOfFile,
        "MapViewOfFile",
        &[P, D, D, D, P],
        R,
        true,
    ),
    section(
        FileApi::MapViewOfFileEx,
        "MapViewOfFileEx",
        &[P, D, D, D, P, P],
        R,
        true,
    ),
    section(FileApi::UnmapViewOfFile, "UnmapViewOfFile", &[P], B, true),
    section(
        FileApi::FlushViewOfFile,
        "FlushViewOfFile",
        &[P, P],
        B,
        true,
    ),
    Contract::new(
        ApiId::File(FileApi::AreFileApisAnsi), "AreFileApisANSI", &[], B,
        "PE32 original kernel32 ABI-only return; Win64 reports the existing ANSI file codepage policy without reading memory or changing LastError",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::LegacyUnimplemented },
         ArchitecturePlan { dlls: &["kernel32.dll", "kernelbase.dll"], execution: ExecutionPolicy::Typed }],
    ),
    endpoint(FileApi::CreatePipe, "CreatePipe", &[P, P, P, D]),
    endpoint(FileApi::GetConsoleMode, "GetConsoleMode", &[P, P]),
    endpoint(FileApi::WriteConsoleW, "WriteConsoleW", &[P, P, D, P, P]),
    Contract::new(
        ApiId::File(FileApi::NtQueryDirectory), "NtQueryDirectoryFile",
        &[P, P, P, P, P, P, D, D, Parameter::Uint8, P, Parameter::Uint8], ReturnConvention::Ntstatus,
        "existing synchronous ntdll directory batches; two BOOLEAN adapters consume low bytes while trace keeps full carriers, only length/class truncate to DWORD; checked IOSB/pattern/output and cursor commit order, no LastError writes; PE32 unknown ABI remains absent",
        [ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent },
         ArchitecturePlan { dlls: &["ntdll.dll"], execution: ExecutionPolicy::Typed }],
    ),
];
