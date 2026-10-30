use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellApi {
    Arguments,
    KnownFolder,
    SpecialFolderA,
    SpecialFolderW,
    FolderA,
    FolderW,
    ExecuteA,
    ExecuteW,
    FileOperationA,
    FileOperationW,
    ExecuteExA,
    ExecuteExW,
}
impl ShellApi {
    pub(crate) fn wide(self) -> bool {
        matches!(self, Self::SpecialFolderW | Self::FolderW)
    }
    pub(crate) fn hresult(self) -> bool {
        matches!(self, Self::KnownFolder | Self::FolderA | Self::FolderW)
    }
}
const fn declaration(
    id: ShellApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    let win64 = matches!(id, ShellApi::Arguments | ShellApi::KnownFolder);
    Contract::new(ApiId::Shell(id), name, parameters, returns,
        "PE32 shell32 directory shim retains C drive root, ignored folder/flags/token, process heap and original output/error order; argv ABI-only; Win64 quoted argv LocalFree blocks and confined known-folder CoTaskMemFree blocks; static providers include legacy kernel32/kernelbase/shell32 but fresh dynamic lookup remains unavailable until a gateway exists",
        [ArchitecturePlan { dlls: &["shell32.dll"], execution: if matches!(id, ShellApi::Arguments) { ExecutionPolicy::LegacyUnimplemented } else { ExecutionPolicy::Typed } },
         ArchitecturePlan { dlls: if win64 { &["kernel32.dll", "kernelbase.dll", "shell32.dll"] } else { &[] }, execution: if win64 { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } }])
        .with_lookup_dlls([&["shell32.dll"], &[]])
}
use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Hresult as H, Pointer as R};
pub static SHELL_CONTRACTS: [Contract; 12] = [
    unavailable(ShellApi::ExecuteExA, "ShellExecuteExA", &[P], B),
    unavailable(ShellApi::ExecuteExW, "ShellExecuteExW", &[P], B),
    unavailable(ShellApi::ExecuteA, "ShellExecuteA", &[P, P, P, P, P, D], R),
    unavailable(ShellApi::ExecuteW, "ShellExecuteW", &[P, P, P, P, P, D], R),
    unavailable(
        ShellApi::FileOperationA,
        "SHFileOperationA",
        &[P],
        ReturnConvention::Uint32,
    ),
    unavailable(
        ShellApi::FileOperationW,
        "SHFileOperationW",
        &[P],
        ReturnConvention::Uint32,
    ),
    declaration(ShellApi::Arguments, "CommandLineToArgvW", &[P, P], R),
    declaration(
        ShellApi::KnownFolder,
        "SHGetKnownFolderPath",
        &[P, D, P, P],
        H,
    ),
    declaration(
        ShellApi::SpecialFolderA,
        "SHGetSpecialFolderPathA",
        &[P, P, D, D],
        B,
    ),
    declaration(
        ShellApi::SpecialFolderW,
        "SHGetSpecialFolderPathW",
        &[P, P, D, D],
        B,
    ),
    declaration(ShellApi::FolderA, "SHGetFolderPathA", &[P, D, P, D, P], H),
    declaration(ShellApi::FolderW, "SHGetFolderPathW", &[P, D, P, D, P], H),
];

const fn unavailable(
    id: ShellApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(ApiId::Shell(id), name, parameters, returns,
        "No shell verb association or interactive/multi-file operation backend; execute returns SE_ERR_NOASSOC and LastError 1155, file operations return ERROR_CALL_NOT_IMPLEMENTED without changing the record, files or LastError",
        [ArchitecturePlan { dlls: &["shell32.dll"], execution: ExecutionPolicy::LegacyUnimplemented }; 2])
}
