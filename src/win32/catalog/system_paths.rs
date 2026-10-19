//! Virtual Windows/system/temp directory query ABI and capability boundaries.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemPathApi {
    SystemA,
    SystemW,
    WindowsA,
    WindowsW,
    SystemWindowsA,
    SystemWindowsW,
    TempA,
    TempW,
    Temp2A,
    Temp2W,
}

impl SystemPathApi {
    pub(crate) fn wide(self) -> bool {
        matches!(
            self,
            Self::SystemW | Self::WindowsW | Self::SystemWindowsW | Self::TempW | Self::Temp2W
        )
    }

    pub(crate) const fn temporary(self) -> bool {
        matches!(
            self,
            Self::TempA | Self::TempW | Self::Temp2A | Self::Temp2W
        )
    }

    pub(crate) fn system(self) -> bool {
        matches!(self, Self::SystemA | Self::SystemW)
    }
}

const fn declaration(id: SystemPathApi, name: &'static str) -> Contract {
    let win64 = !matches!(id, SystemPathApi::TempA | SystemPathApi::Temp2A);
    use Parameter::{Pointer as P, Uint32 as D};
    Contract::new(
        ApiId::SystemPath(id), name,
        if id.temporary() { &[D, P] } else { &[P, D] }, ReturnConvention::CharacterCount,
        "existing immutable Windows/System32/Temp defaults, NUL-inclusive required capacity and NUL-exclusive success count, A/W character units, original pointer/count argument orders, insufficient capacity leaves output and LastError intact; no directory creation or access test; PE32 kernel32 remains ABI-only, Win64 ANSI temp queries remain absent",
        [ArchitecturePlan { dlls: &["kernel32.dll"], execution: ExecutionPolicy::LegacyUnimplemented },
         ArchitecturePlan { dlls: if win64 { &["kernel32.dll", "kernelbase.dll"] } else { &[] }, execution: if win64 { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } }],
    )
}

pub static SYSTEM_PATH_CONTRACTS: [Contract; 10] = [
    declaration(SystemPathApi::SystemA, "GetSystemDirectoryA"),
    declaration(SystemPathApi::SystemW, "GetSystemDirectoryW"),
    declaration(SystemPathApi::WindowsA, "GetWindowsDirectoryA"),
    declaration(SystemPathApi::WindowsW, "GetWindowsDirectoryW"),
    declaration(SystemPathApi::SystemWindowsA, "GetSystemWindowsDirectoryA"),
    declaration(SystemPathApi::SystemWindowsW, "GetSystemWindowsDirectoryW"),
    declaration(SystemPathApi::TempA, "GetTempPathA"),
    declaration(SystemPathApi::TempW, "GetTempPathW"),
    declaration(SystemPathApi::Temp2A, "GetTempPath2A"),
    declaration(SystemPathApi::Temp2W, "GetTempPath2W"),
];
