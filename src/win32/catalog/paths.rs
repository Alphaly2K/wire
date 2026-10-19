use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathApi {
    FileExistsA,
    FileExistsW,
    IsDirectoryA,
    IsDirectoryW,
    RemoveFileSpecA,
    RemoveFileSpecW,
    RenameExtensionA,
    RenameExtensionW,
    AddExtensionA,
    AddExtensionW,
    UnquoteSpacesA,
    UnquoteSpacesW,
    MakeSureDirectoryPathExists,
    UniversalNameA,
    UniversalNameW,
}

impl PathApi {
    pub(crate) fn wide(self) -> bool {
        matches!(
            self,
            Self::FileExistsW
                | Self::IsDirectoryW
                | Self::RemoveFileSpecW
                | Self::RenameExtensionW
                | Self::AddExtensionW
                | Self::UnquoteSpacesW
                | Self::UniversalNameW
        )
    }
}

const fn declaration(
    id: PathApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Path(id), name, parameters, returns,
        "PE32 checked CP932/UTF-16 path helpers; pinned root rules, MAX_PATH extension limit, NULL short-circuits and output/LastError order; unquote retains zero EAX carrier; confined imagehlp directory creation; MPR universal names retain local-drive ERROR_NOT_CONNECTED with ignored pointers and unchanged LastError; absent on Win64",
        [
            ArchitecturePlan {
                dlls: if matches!(id, PathApi::MakeSureDirectoryPathExists) { &["imagehlp.dll"] } else if matches!(id, PathApi::UniversalNameA | PathApi::UniversalNameW) { &["mpr.dll"] } else { &["shlwapi.dll"] },
                execution: ExecutionPolicy::Typed,
            },
            ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent },
        ],
    )
}

use Parameter::{Pointer as P, Uint32 as D};
use ReturnConvention::{BoolLastError as B, Void};

pub static PATH_CONTRACTS: [Contract; 15] = [
    declaration(
        PathApi::UniversalNameA,
        "WNetGetUniversalNameA",
        &[P, D, P, P],
        ReturnConvention::Uint32,
    ),
    declaration(
        PathApi::UniversalNameW,
        "WNetGetUniversalNameW",
        &[P, D, P, P],
        ReturnConvention::Uint32,
    ),
    declaration(PathApi::FileExistsA, "PathFileExistsA", &[P], B),
    declaration(PathApi::FileExistsW, "PathFileExistsW", &[P], B),
    declaration(PathApi::IsDirectoryA, "PathIsDirectoryA", &[P], B),
    declaration(PathApi::IsDirectoryW, "PathIsDirectoryW", &[P], B),
    declaration(PathApi::RemoveFileSpecA, "PathRemoveFileSpecA", &[P], B),
    declaration(PathApi::RemoveFileSpecW, "PathRemoveFileSpecW", &[P], B),
    declaration(
        PathApi::RenameExtensionA,
        "PathRenameExtensionA",
        &[P, P],
        B,
    ),
    declaration(
        PathApi::RenameExtensionW,
        "PathRenameExtensionW",
        &[P, P],
        B,
    ),
    declaration(PathApi::AddExtensionA, "PathAddExtensionA", &[P, P], B),
    declaration(PathApi::AddExtensionW, "PathAddExtensionW", &[P, P], B),
    declaration(PathApi::UnquoteSpacesA, "PathUnquoteSpacesA", &[P], Void),
    declaration(PathApi::UnquoteSpacesW, "PathUnquoteSpacesW", &[P], Void),
    declaration(
        PathApi::MakeSureDirectoryPathExists,
        "MakeSureDirectoryPathExists",
        &[P],
        B,
    ),
];
