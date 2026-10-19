//! Window-class registration/query contracts; Win64 metadata queries remain absent.
use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassApi {
    RegisterA,
    RegisterW,
    RegisterExA,
    RegisterExW,
    UnregisterA,
    UnregisterW,
    InfoA,
    InfoW,
    InfoExA,
    InfoExW,
    LongA,
    LongW,
    SetLongA,
    SetLongW,
    Word,
    NameA,
    NameW,
    InitializeControls,
    InitializeControlsEx,
}

impl ClassApi {
    pub(crate) fn wide(self) -> bool {
        matches!(
            self,
            Self::RegisterW
                | Self::RegisterExW
                | Self::UnregisterW
                | Self::InfoW
                | Self::InfoExW
                | Self::LongW
                | Self::SetLongW
                | Self::NameW
        )
    }
    pub(crate) fn extended(self) -> bool {
        matches!(
            self,
            Self::RegisterExA | Self::RegisterExW | Self::InfoExA | Self::InfoExW
        )
    }
    pub(crate) fn register(self) -> bool {
        matches!(
            self,
            Self::RegisterA | Self::RegisterW | Self::RegisterExA | Self::RegisterExW
        )
    }
    pub(crate) fn unregister(self) -> bool {
        matches!(self, Self::UnregisterA | Self::UnregisterW)
    }
    pub(crate) const fn query(self) -> bool {
        matches!(
            self,
            Self::InfoA | Self::InfoW | Self::InfoExA | Self::InfoExW
        )
    }
    pub(crate) const fn win64(self) -> bool {
        matches!(
            self,
            Self::RegisterA
                | Self::RegisterW
                | Self::RegisterExA
                | Self::RegisterExW
                | Self::UnregisterA
                | Self::UnregisterW
        )
    }
    pub(crate) fn field(self) -> bool {
        matches!(
            self,
            Self::LongA | Self::LongW | Self::SetLongA | Self::SetLongW | Self::Word
        )
    }
    pub(crate) fn set(self) -> bool {
        matches!(self, Self::SetLongA | Self::SetLongW)
    }
}

const fn declaration(
    id: ClassApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Class(id), name, parameters, returns,
        "retain architecture-specific class layout/name limit/global matching/menu and extra-storage rules, monotonic atom boundaries, persistent PE32 A/W names and implicit control registration; preserve validation/fault/LastError/return-frame and allocation commit order; Win64 metadata queries remain absent and kernel aliases remain static-only",
        [ArchitecturePlan { dlls: &["user32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: if id.win64() { &["kernel32.dll", "kernelbase.dll", "user32.dll"] } else { &[] }, execution: if id.win64() { ExecutionPolicy::Typed } else { ExecutionPolicy::Absent } }],
    ).with_lookup_dlls([&["user32.dll"], if id.win64() { &["user32.dll"] } else { &[] }])
}

use Parameter::{Int32 as I, Pointer as P};
use ReturnConvention::{BoolLastError as B, Uint32 as U};
const fn initialization(
    id: ClassApi,
    name: &'static str,
    parameters: &'static [Parameter],
    returns: ReturnConvention,
) -> Contract {
    Contract::new(
        ApiId::Class(id), name, parameters, returns,
        "PE32 basic global window/text classes only; Ex reads dwSize alone and ignores dwICC; invalid size retains error 87 and empty note; preserve ordered partial registration, existing classes, LastError, scalar return 1 and trace; Win64 initialization remains absent",
        [ArchitecturePlan { dlls: &["comctl32.dll"], execution: ExecutionPolicy::Typed },
         ArchitecturePlan { dlls: &[], execution: ExecutionPolicy::Absent }],
    )
}
pub static CLASS_CONTRACTS: [Contract; 19] = [
    declaration(ClassApi::RegisterA, "RegisterClassA", &[P], U),
    declaration(ClassApi::RegisterW, "RegisterClassW", &[P], U),
    declaration(ClassApi::RegisterExA, "RegisterClassExA", &[P], U),
    declaration(ClassApi::RegisterExW, "RegisterClassExW", &[P], U),
    declaration(ClassApi::UnregisterA, "UnregisterClassA", &[P, P], B),
    declaration(ClassApi::UnregisterW, "UnregisterClassW", &[P, P], B),
    declaration(ClassApi::InfoA, "GetClassInfoA", &[P, P, P], B),
    declaration(ClassApi::InfoW, "GetClassInfoW", &[P, P, P], B),
    declaration(ClassApi::InfoExA, "GetClassInfoExA", &[P, P, P], B),
    declaration(ClassApi::InfoExW, "GetClassInfoExW", &[P, P, P], B),
    declaration(ClassApi::LongA, "GetClassLongA", &[P, I], U),
    declaration(ClassApi::LongW, "GetClassLongW", &[P, I], U),
    declaration(ClassApi::SetLongA, "SetClassLongA", &[P, I, P], U),
    declaration(ClassApi::SetLongW, "SetClassLongW", &[P, I, P], U),
    declaration(ClassApi::Word, "GetClassWord", &[P, I], U),
    declaration(ClassApi::NameA, "GetClassNameA", &[P, P, I], U),
    declaration(ClassApi::NameW, "GetClassNameW", &[P, P, I], U),
    initialization(ClassApi::InitializeControls, "InitCommonControls", &[], U),
    initialization(
        ClassApi::InitializeControlsEx,
        "InitCommonControlsEx",
        &[P],
        B,
    ),
];
