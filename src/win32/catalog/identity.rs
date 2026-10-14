use super::{ApiId, ArchitecturePlan, Contract, ExecutionPolicy, Parameter, ReturnConvention};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentityApi {
    ComputerNameA,
    ComputerNameW,
    ComputerNameExA,
    ComputerNameExW,
    UserNameA,
    UserNameW,
}
impl IdentityApi {
    pub const fn user(self) -> bool {
        matches!(self, Self::UserNameA | Self::UserNameW)
    }
    pub const fn extended(self) -> bool {
        matches!(self, Self::ComputerNameExA | Self::ComputerNameExW)
    }
    pub const fn wide(self) -> bool {
        matches!(
            self,
            Self::ComputerNameW | Self::ComputerNameExW | Self::UserNameW
        )
    }
}

const fn declaration(
    id: IdentityApi,
    name: &'static str,
    parameters: &'static [Parameter],
) -> Contract {
    Contract::new(
        ApiId::Identity(id), name, parameters, ReturnConvention::BoolLastError,
        "isolated Win64 identity; PE32 keeps known ABI and legacy unimplemented policy, including historical kernel32 GetUserName lookup",
        [
            ArchitecturePlan {
                dlls: if id.user() { &["advapi32.dll", "kernel32.dll"] } else { &["kernel32.dll"] },
                execution: ExecutionPolicy::LegacyUnimplemented,
            },
            ArchitecturePlan {
                dlls: if id.user() { &["advapi32.dll"] } else { &["kernel32.dll", "kernelbase.dll"] },
                execution: ExecutionPolicy::Typed,
            },
        ],
    )
}

pub static IDENTITY_CONTRACTS: [Contract; 6] = [
    declaration(
        IdentityApi::ComputerNameA,
        "GetComputerNameA",
        &[Parameter::Pointer, Parameter::Pointer],
    ),
    declaration(
        IdentityApi::ComputerNameW,
        "GetComputerNameW",
        &[Parameter::Pointer, Parameter::Pointer],
    ),
    declaration(
        IdentityApi::ComputerNameExA,
        "GetComputerNameExA",
        &[Parameter::Uint32, Parameter::Pointer, Parameter::Pointer],
    ),
    declaration(
        IdentityApi::ComputerNameExW,
        "GetComputerNameExW",
        &[Parameter::Uint32, Parameter::Pointer, Parameter::Pointer],
    ),
    declaration(
        IdentityApi::UserNameA,
        "GetUserNameA",
        &[Parameter::Pointer, Parameter::Pointer],
    ),
    declaration(
        IdentityApi::UserNameW,
        "GetUserNameW",
        &[Parameter::Pointer, Parameter::Pointer],
    ),
];
